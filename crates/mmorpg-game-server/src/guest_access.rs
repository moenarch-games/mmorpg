//! Game-owned, world-scoped guest entitlements. Only public keys are stored on the host;
//! setup membership never grants a character, and clients never choose its durable ID.
//!
//! An unknown key's request lives only as long as its admission wait (#1144): it ends
//! when the wait times out, is cancelled or is decided, so unanswered requests never jam
//! the bounded pending set. Deny is transient; only Block or Remove persist a revoked
//! entitlement, and the owner can Forget a revoked one to free its stored entry.
use ring::signature::{ED25519, UnparsedPublicKey};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs::{self, File, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::Duration,
};

/// Distinct unknown keys that may wait for the owner at once.
const MAX_PENDING: usize = 16;
/// Persisted entitlements (admitted, removed or blocked keys).
const MAX_ENTRIES: usize = 1024;
/// Polls of 100 ms each; game-server bounds the wait further.
const WAIT_POLLS: u32 = 750;

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Entitlement {
    character: String,
    revoked: bool,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct GuestView {
    pub key: String,
    pub code: String,
    pub pending: bool,
    pub revoked: bool,
}

/// The owner's answer to a guest key, from the private local host-control surface.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GuestDecision {
    /// Admit a waiting key, or restore a removed one with its existing character.
    Approve,
    /// Refuse a waiting request without storing anything; the key may ask again.
    Deny,
    /// Persistently refuse a waiting or admitted key. Progression is retained.
    Block,
    /// Delete a removed or blocked key's stored entry. Admitted keys must be removed
    /// first; a forgotten key that asks again is an unknown guest.
    Forget,
}

impl GuestDecision {
    #[must_use]
    pub fn word(self) -> &'static str {
        match self {
            Self::Approve => "approve",
            Self::Deny => "deny",
            Self::Block => "remove",
            Self::Forget => "forget",
        }
    }

    #[must_use]
    pub fn parse(word: &str) -> Option<Self> {
        [Self::Approve, Self::Deny, Self::Block, Self::Forget]
            .into_iter()
            .find(|decision| decision.word() == word)
    }
}

#[derive(Default)]
struct Request {
    /// Admission waits currently held for this key.
    waiters: usize,
    /// Denied by the owner: every current wait ends refused, then the request is gone.
    denied: bool,
}

struct Inner {
    path: PathBuf,
    entries: BTreeMap<String, Entitlement>,
    pending: BTreeMap<String, Request>,
    sessions: BTreeMap<String, String>,
}

#[derive(Clone)]
pub struct GuestAccess(Arc<Mutex<Inner>>);

/// Holds one admission wait's place in the pending set; dropping it, whether the wait
/// finished or its future was cancelled, withdraws the request once no wait remains.
struct Wait {
    access: GuestAccess,
    key: String,
}

impl Drop for Wait {
    fn drop(&mut self) {
        let Ok(mut inner) = self.access.0.lock() else {
            return;
        };
        if let Some(request) = inner.pending.get_mut(&self.key) {
            request.waiters = request.waiters.saturating_sub(1);
            if request.waiters == 0 {
                inner.pending.remove(&self.key);
            }
        }
    }
}

pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

impl GuestAccess {
    pub fn open(directory: &Path) -> Result<Self, String> {
        let path = directory.join("guest-access.json");
        let entries: BTreeMap<String, Entitlement> = match fs::read(&path) {
            Ok(bytes) if bytes.len() <= 1_048_576 => {
                serde_json::from_slice(&bytes).map_err(|e| e.to_string())?
            }
            Ok(_) => return Err("Guest access file is too large.".into()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => BTreeMap::new(),
            Err(e) => return Err(e.to_string()),
        };
        if entries.len() > MAX_ENTRIES
            || entries.iter().any(|(key, entry)| {
                key.len() != 64
                    || !key.bytes().all(|b| b.is_ascii_hexdigit())
                    || mmorpg_core::CharacterId::new(entry.character.clone()).is_err()
            })
        {
            return Err("Invalid guest access file; saved characters were retained.".into());
        }
        Ok(Self(Arc::new(Mutex::new(Inner {
            path,
            entries,
            pending: BTreeMap::new(),
            sessions: BTreeMap::new(),
        }))))
    }

    /// A signature proves possession of the guest key without transmitting a reusable
    /// secret. Unknown keys await a decision from the private local host-control surface.
    pub async fn authorize(
        &self,
        lobby: String,
        matched: String,
        participant: String,
        proof: Vec<u8>,
    ) -> bool {
        self.authorize_within(lobby, matched, participant, proof, WAIT_POLLS)
            .await
    }

    async fn authorize_within(
        &self,
        lobby: String,
        matched: String,
        participant: String,
        proof: Vec<u8>,
        polls: u32,
    ) -> bool {
        if proof.len() != 96
            || UnparsedPublicKey::new(&ED25519, &proof[..32])
                .verify(
                    &crate::invitation::admission_message(&lobby, &participant, &matched),
                    &proof[32..],
                )
                .is_err()
        {
            return false;
        }
        let key = hex(&proof[..32]);
        let mut wait = None;
        for _ in 0..polls {
            {
                let Ok(mut inner) = self.0.lock() else {
                    return false;
                };
                if let Some(entry) = inner.entries.get(&key) {
                    if entry.revoked {
                        return false;
                    }
                    inner.sessions.insert(participant.clone(), key.clone());
                    return true;
                }
                if wait.is_none() {
                    if !inner.pending.contains_key(&key) && inner.pending.len() >= MAX_PENDING {
                        return false;
                    }
                    inner.pending.entry(key.clone()).or_default().waiters += 1;
                    wait = Some(Wait {
                        access: self.clone(),
                        key: key.clone(),
                    });
                }
                if inner
                    .pending
                    .get(&key)
                    .is_some_and(|request| request.denied)
                {
                    return false;
                }
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        false
    }

    pub fn character(&self, participant: &str) -> Option<mmorpg_core::CharacterId> {
        let inner = self.0.lock().ok()?;
        let entry = inner.entries.get(inner.sessions.get(participant)?)?;
        (!entry.revoked)
            .then(|| mmorpg_core::CharacterId::new(entry.character.clone()).ok())
            .flatten()
    }

    pub fn guests(&self) -> Vec<GuestView> {
        let Ok(inner) = self.0.lock() else {
            return Vec::new();
        };
        inner
            .pending
            .iter()
            // Decided requests are listed by their entry, or not at all once denied.
            .filter(|(key, request)| !request.denied && !inner.entries.contains_key(*key))
            .map(|(key, _)| GuestView {
                key: key.clone(),
                code: key[..12].into(),
                pending: true,
                revoked: false,
            })
            .chain(inner.entries.iter().map(|(key, entry)| GuestView {
                key: key.clone(),
                code: key[..12].into(),
                pending: false,
                revoked: entry.revoked,
            }))
            .collect()
    }

    /// Local owner action only. Reapproval retains the existing durable character ID.
    pub fn decide(&self, key: &str, decision: GuestDecision) -> Result<(), String> {
        let mut inner = self.0.lock().map_err(|_| "Guest access unavailable")?;
        let waiting = inner
            .pending
            .get(key)
            .is_some_and(|request| !request.denied);
        let mut entries = inner.entries.clone();
        match (decision, entries.get_mut(key)) {
            (GuestDecision::Deny, None) => {
                let request = inner
                    .pending
                    .get_mut(key)
                    .filter(|request| !request.denied)
                    .ok_or("Guest request is no longer pending.")?;
                request.denied = true;
                return Ok(());
            }
            (GuestDecision::Deny, Some(_)) => {
                return Err("This guest was already decided; use Remove instead.".into());
            }
            (GuestDecision::Forget, Some(entry)) if entry.revoked => {
                entries.remove(key);
            }
            (GuestDecision::Forget, Some(_)) => {
                return Err("Remove this guest before forgetting the key.".into());
            }
            (GuestDecision::Forget, None) => return Err("Unknown guest key.".into()),
            (GuestDecision::Approve | GuestDecision::Block, Some(entry)) => {
                entry.revoked = decision == GuestDecision::Block;
            }
            (GuestDecision::Approve | GuestDecision::Block, None) => {
                if !waiting {
                    return Err("Guest request is no longer pending.".into());
                }
                if entries.len() >= MAX_ENTRIES {
                    return Err("World guest limit reached. Forget a removed guest first.".into());
                }
                let mut random = [0; 16];
                getrandom::fill(&mut random).map_err(|e| e.to_string())?;
                entries.insert(
                    key.into(),
                    Entitlement {
                        character: format!("guest-{}", hex(&random)),
                        revoked: decision == GuestDecision::Block,
                    },
                );
            }
        }
        persist(&inner.path, &entries)?;
        inner.entries = entries;
        Ok(())
    }
}

fn persist(path: &Path, entries: &BTreeMap<String, Entitlement>) -> Result<(), String> {
    let temporary = path.with_extension("tmp");
    let mut options = OpenOptions::new();
    options.create(true).truncate(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(&temporary).map_err(|e| e.to_string())?;
    file.write_all(&serde_json::to_vec(entries).map_err(|e| e.to_string())?)
        .and_then(|()| file.sync_all())
        .map_err(|e| e.to_string())?;
    fs::rename(temporary, path).map_err(|e| e.to_string())?;
    #[cfg(unix)]
    File::open(path.parent().ok_or("Missing guest store directory")?)
        .and_then(|f| f.sync_all())
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use ring::{
        rand::SystemRandom,
        signature::{Ed25519KeyPair, KeyPair},
    };

    const LOBBY: &str = "lobby";
    const MATCHED: &str = "zone-1";

    struct Guest(Ed25519KeyPair);

    impl Guest {
        fn new() -> Self {
            let pkcs8 = Ed25519KeyPair::generate_pkcs8(&SystemRandom::new()).unwrap();
            Self(Ed25519KeyPair::from_pkcs8(pkcs8.as_ref()).unwrap())
        }
        fn key(&self) -> String {
            hex(self.0.public_key().as_ref())
        }
        fn proof(&self, participant: &str) -> Vec<u8> {
            let mut proof = self.0.public_key().as_ref().to_vec();
            proof.extend_from_slice(
                self.0
                    .sign(&crate::invitation::admission_message(
                        LOBBY,
                        participant,
                        MATCHED,
                    ))
                    .as_ref(),
            );
            proof
        }
        /// One admission wait of `polls` × 100 ms, as the host's authorizer runs it.
        fn ask(
            &self,
            access: &GuestAccess,
            participant: &str,
            polls: u32,
        ) -> tokio::task::JoinHandle<bool> {
            let access = access.clone();
            let participant = participant.to_owned();
            let proof = self.proof(&participant);
            tokio::spawn(async move {
                access
                    .authorize_within(LOBBY.into(), MATCHED.into(), participant, proof, polls)
                    .await
            })
        }
    }

    fn waiting(access: &GuestAccess) -> Vec<String> {
        access
            .guests()
            .into_iter()
            .filter(|guest| guest.pending)
            .map(|guest| guest.key)
            .collect()
    }

    fn stored(directory: &Path) -> usize {
        fs::read(directory.join("guest-access.json")).map_or(0, |bytes| {
            serde_json::from_slice::<BTreeMap<String, Entitlement>>(&bytes)
                .unwrap()
                .len()
        })
    }

    async fn until(mut done: impl FnMut() -> bool) {
        for _ in 0..1000 {
            if done() {
                return;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        panic!("condition not reached");
    }

    #[tokio::test]
    async fn requests_that_fill_the_pending_set_expire_and_a_later_guest_is_admitted() {
        let directory = tempfile::tempdir().unwrap();
        let access = GuestAccess::open(directory.path()).unwrap();
        let jammers: Vec<_> = (0..MAX_PENDING).map(|_| Guest::new()).collect();
        // Half of the requests time out after their wait; half disconnect mid-wait.
        let mut waits: Vec<_> = jammers
            .iter()
            .enumerate()
            .map(|(i, guest)| guest.ask(&access, &format!("jammer-{i}"), 5))
            .collect();
        let disconnected: Vec<_> = (0..MAX_PENDING / 2)
            .map(|i| {
                jammers[i]
                    .ask(&access, &format!("jammer-{i}-again"), WAIT_POLLS)
                    .abort_handle()
            })
            .collect();
        until(|| waiting(&access).len() == MAX_PENDING).await;
        let late = Guest::new();
        assert!(
            !late.ask(&access, "early", WAIT_POLLS).await.unwrap(),
            "the pending set is full while the waits last"
        );
        for wait in waits.drain(..) {
            assert!(!wait.await.unwrap());
        }
        for abort in disconnected {
            abort.abort();
        }
        until(|| waiting(&access).is_empty()).await;

        let admission = late.ask(&access, "late", WAIT_POLLS);
        until(|| waiting(&access) == [late.key()]).await;
        access.decide(&late.key(), GuestDecision::Approve).unwrap();
        assert!(admission.await.unwrap());
        assert!(access.character("late").is_some());
        assert_eq!(stored(directory.path()), 1, "no jammer was stored");
    }

    #[tokio::test]
    async fn deny_is_transient_and_does_not_consume_the_stored_guest_limit() {
        let directory = tempfile::tempdir().unwrap();
        let access = GuestAccess::open(directory.path()).unwrap();
        let guest = Guest::new();
        let admission = guest.ask(&access, "first", WAIT_POLLS);
        until(|| waiting(&access) == [guest.key()]).await;
        access.decide(&guest.key(), GuestDecision::Deny).unwrap();
        assert!(!admission.await.unwrap());
        assert!(access.guests().is_empty());
        assert_eq!(stored(directory.path()), 0);
        // A fresh store still sees no entry, so the cap is untouched.
        assert!(
            GuestAccess::open(directory.path())
                .unwrap()
                .guests()
                .is_empty()
        );

        // The denied key may ask again and be approved.
        let retry = guest.ask(&access, "second", WAIT_POLLS);
        until(|| waiting(&access) == [guest.key()]).await;
        access.decide(&guest.key(), GuestDecision::Approve).unwrap();
        assert!(retry.await.unwrap());
        assert_eq!(stored(directory.path()), 1);
    }

    #[tokio::test]
    async fn block_persists_and_forget_frees_the_entry() {
        let directory = tempfile::tempdir().unwrap();
        let access = GuestAccess::open(directory.path()).unwrap();
        let guest = Guest::new();
        let admission = guest.ask(&access, "first", WAIT_POLLS);
        until(|| waiting(&access) == [guest.key()]).await;
        access.decide(&guest.key(), GuestDecision::Block).unwrap();
        assert!(!admission.await.unwrap());
        assert_eq!(stored(directory.path()), 1);
        // Blocked keys are refused at once and hold no pending place.
        assert!(!guest.ask(&access, "again", WAIT_POLLS).await.unwrap());
        assert!(waiting(&access).is_empty());

        access.decide(&guest.key(), GuestDecision::Forget).unwrap();
        assert_eq!(stored(directory.path()), 0);
        assert!(access.guests().is_empty());
    }

    #[tokio::test]
    async fn remove_keeps_the_character_and_admitted_keys_cannot_be_forgotten() {
        let directory = tempfile::tempdir().unwrap();
        let access = GuestAccess::open(directory.path()).unwrap();
        let guest = Guest::new();
        let admission = guest.ask(&access, "first", WAIT_POLLS);
        until(|| waiting(&access) == [guest.key()]).await;
        access.decide(&guest.key(), GuestDecision::Approve).unwrap();
        assert!(admission.await.unwrap());
        let character = access.character("first").unwrap();

        assert!(access.decide(&guest.key(), GuestDecision::Forget).is_err());
        assert!(access.decide(&guest.key(), GuestDecision::Deny).is_err());
        access.decide(&guest.key(), GuestDecision::Block).unwrap();
        assert!(access.character("first").is_none());
        access.decide(&guest.key(), GuestDecision::Approve).unwrap();
        // A returning key resumes its character without another approval.
        let reopened = GuestAccess::open(directory.path()).unwrap();
        assert!(guest.ask(&reopened, "return", 1).await.unwrap());
        assert_eq!(reopened.character("return"), Some(character));
    }
}
