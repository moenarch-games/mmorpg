//! Game-owned, world-scoped guest entitlements. Only public keys are stored on the host;
//! setup membership never grants a character, and clients never choose its durable ID.
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

struct Inner {
    path: PathBuf,
    entries: BTreeMap<String, Entitlement>,
    pending: BTreeMap<String, ()>,
    sessions: BTreeMap<String, String>,
}

#[derive(Clone)]
pub struct GuestAccess(Arc<Mutex<Inner>>);

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
        if entries.len() > 1024
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
        for _ in 0..750 {
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
                if inner.pending.len() >= 16 {
                    return false;
                }
                inner.pending.insert(key.clone(), ());
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
            .keys()
            .map(|key| GuestView {
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
    pub fn decide(&self, key: &str, allow: bool) -> Result<(), String> {
        let mut inner = self.0.lock().map_err(|_| "Guest access unavailable")?;
        let mut entries = inner.entries.clone();
        if let Some(entry) = entries.get_mut(key) {
            entry.revoked = !allow;
        } else {
            if !inner.pending.contains_key(key) {
                return Err("Guest request is no longer pending.".into());
            }
            if entries.len() >= 1024 {
                return Err("World guest limit reached.".into());
            }
            let mut random = [0; 16];
            getrandom::fill(&mut random).map_err(|e| e.to_string())?;
            entries.insert(
                key.into(),
                Entitlement {
                    character: format!("guest-{}", hex(&random)),
                    revoked: !allow,
                },
            );
        }
        let temporary = inner.path.with_extension("tmp");
        let mut options = OpenOptions::new();
        options.create(true).truncate(true).write(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&temporary).map_err(|e| e.to_string())?;
        file.write_all(&serde_json::to_vec(&entries).map_err(|e| e.to_string())?)
            .and_then(|()| file.sync_all())
            .map_err(|e| e.to_string())?;
        fs::rename(temporary, &inner.path).map_err(|e| e.to_string())?;
        #[cfg(unix)]
        File::open(inner.path.parent().ok_or("Missing guest store directory")?)
            .and_then(|f| f.sync_all())
            .map_err(|e| e.to_string())?;
        inner.entries = entries;
        inner.pending.remove(key);
        Ok(())
    }
}
