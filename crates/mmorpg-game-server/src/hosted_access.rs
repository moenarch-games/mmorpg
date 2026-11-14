//! Public hosted worlds compose game-owned entitlement with existing character/party
//! control. The development/LAN arbitrary-ID claim remains inaccessible to guests.
use crate::{
    ZoneGameServerAdapter,
    guest_access::{GuestAccess, GuestView},
    invitation::JoinInvite,
};
use game_server::webrtc_transport::{Lobby, WebRtcControlService, WebRtcHost, WebRtcHostConfig};
use game_server::{
    ControlContext, ControlServiceError, LiveMatchHost, MatchControlService, MatchId,
};
use ring::hmac;
use serde::{Deserialize, Serialize};
use std::{
    path::Path,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

#[derive(Clone, Default, Serialize, Deserialize)]
pub struct ShareStatus {
    pub invitation: Option<String>,
    pub problem: Option<String>,
    pub guests: Vec<GuestView>,
}

#[derive(Clone)]
pub struct ManagedWorld {
    pub access: GuestAccess,
    pub owner_key: hmac::Key,
    shared: Arc<Mutex<ShareStatus>>,
    requested: Arc<AtomicBool>,
}

impl ManagedWorld {
    pub fn open(directory: &Path, token: &[u8]) -> Result<Self, String> {
        Ok(Self {
            access: GuestAccess::open(directory)?,
            owner_key: hmac::Key::new(hmac::HMAC_SHA256, token),
            shared: Arc::default(),
            requested: Arc::new(AtomicBool::new(true)),
        })
    }
    pub fn status(&self) -> ShareStatus {
        let mut status = self
            .shared
            .lock()
            .map(|s| s.clone())
            .unwrap_or_else(|_| ShareStatus {
                problem: Some("Hosting status unavailable. Check host.log.".into()),
                ..ShareStatus::default()
            });
        status.guests = self.access.guests();
        status
    }
    pub fn request_share(&self) {
        self.requested.store(true, Ordering::Relaxed);
    }
    pub fn clear(&self, problem: Option<String>) {
        if let Ok(mut status) = self.shared.lock() {
            status.invitation = None;
            status.problem = problem;
        }
    }

    pub async fn run<C>(&self, host: LiveMatchHost<ZoneGameServerAdapter>, control: Arc<C>)
    where
        C: WebRtcControlService + 'static,
    {
        // Read/verify once outside the simulation lock; every peer sees one immutable release.
        let content = crate::shared_assets::installed_directory().and_then(|path| {
            crate::shared_assets::Catalogue::open(
                &path,
                mmorpg_core::content_for_zone(mmorpg_core::ZoneId::new(1)).revision(),
            )
        });
        if let Err(error) = &content {
            eprintln!(
                "Optional host detail unavailable: {error}. Install assets/ambient or set MMORPG_SHARED_ART_DIR."
            );
        }
        let content = Arc::new(crate::shared_assets::ContentService(content));
        loop {
            tokio::time::sleep(Duration::from_millis(100)).await;
            if !host.is_serving() || !self.requested.swap(false, Ordering::Relaxed) {
                continue;
            }
            if std::env::var_os("MMORPG_TEST_OFFLINE_HOST").is_some() {
                continue;
            }
            self.clear(Some("Registering the ready world…".into()));
            let result = async {
                let lobby = Lobby::create(crate::invitation::setup_config()?, 16)
                    .await
                    .map_err(|e| e.to_string())?;
                let lobby_id = lobby.lobby_id().to_owned();
                let matched =
                    crate::zone_match_id(mmorpg_core::ZoneId::new(1)).map_err(|e| e.to_string())?;
                let authorizer = self.access.clone();
                let validator = self.access.clone();
                let match_text = matched.as_str().to_owned();
                let rtc_control: Arc<dyn WebRtcControlService> = control.clone();
                let rtc = WebRtcHost::attach(
                    lobby,
                    host.clone(),
                    matched.clone(),
                    rtc_control,
                    WebRtcHostConfig {
                        bind_ip: crate::invitation::bind_ip()?,
                        relay_only: false,
                    },
                )
                .map_err(|e| e.to_string())?
                .with_content_service(content.clone());
                if let Ok(mut status) = self.shared.lock() {
                    status.invitation = Some(
                        JoinInvite {
                            lobby_id: lobby_id.clone(),
                            match_id: matched,
                        }
                        .url(),
                    );
                    status.problem = None;
                }
                rtc.run_with_authorization(
                    move |participant, proof| {
                        let access = authorizer.clone();
                        let lobby = lobby_id.clone();
                        let matched = match_text.clone();
                        async move { access.authorize(lobby, matched, participant, proof).await }
                    },
                    move |participant| validator.character(participant).is_some(),
                    std::future::pending(),
                )
                .await
                .map_err(|e| e.to_string())
            }
            .await;
            self.clear(Some(match result { Ok(()) => "Sharing stopped. Start sharing again to create a current invitation.".into(), Err(error) => format!("Sharing unavailable: {error}. Check the network and retry sharing. Previous invitations may be expired or unavailable.") }));
        }
    }
}

pub(crate) struct ScopedControl<C> {
    inner: Arc<C>,
    managed: Option<ManagedWorld>,
}
impl<C> Clone for ScopedControl<C> {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
            managed: self.managed.clone(),
        }
    }
}
impl<C> ScopedControl<C> {
    pub fn new(inner: C, managed: Option<ManagedWorld>) -> Self {
        Self {
            inner: Arc::new(inner),
            managed,
        }
    }
}

impl<C: MatchControlService> MatchControlService for ScopedControl<C> {
    fn handle(
        &self,
        id: &MatchId,
        context: ControlContext,
        payload: &[u8],
    ) -> Result<Vec<u8>, ControlServiceError> {
        if let Some(managed) = &self.managed {
            if let [1, 3, signature_and_body @ ..] = payload {
                if signature_and_body.len() < 32 {
                    return Err(ControlServiceError::new("Invalid owner claim"));
                }
                let (signature, body) = signature_and_body.split_at(32);
                hmac::verify(
                    &managed.owner_key,
                    &{
                        let mut message = context.player_id.to_be_bytes().to_vec();
                        message.extend_from_slice(&context.connection_epoch.to_be_bytes());
                        message.extend_from_slice(body);
                        message
                    },
                    signature,
                )
                .map_err(|_| ControlServiceError::new("Owner authorization required"))?;
                if !body.starts_with(&[1, 1]) {
                    return Err(ControlServiceError::new("Invalid owner operation"));
                }
                return self.inner.handle(id, context, body);
            }
            if payload.starts_with(&[1, 1]) {
                return Err(ControlServiceError::new(
                    "Managed worlds require authenticated character admission",
                ));
            }
        }
        self.inner.handle(id, context, payload)
    }
}

impl<C: MatchControlService> WebRtcControlService for ScopedControl<C> {
    fn handle(
        &self,
        participant: &str,
        id: &MatchId,
        context: ControlContext,
        payload: &[u8],
    ) -> Result<Vec<u8>, ControlServiceError> {
        let character = self
            .managed
            .as_ref()
            .and_then(|m| m.access.character(participant))
            .ok_or_else(|| ControlServiceError::new("Guest access revoked"))?;
        if let [1, 1, appearance, ..] = payload {
            let mut claim = vec![1, 1, *appearance];
            claim.extend_from_slice(character.as_str().as_bytes());
            // The authenticated participant's entitlement supplies the identity. Any
            // client-supplied CharacterId is deliberately ignored.
            return self.inner.handle(id, context, &claim);
        }
        if payload.starts_with(&[1, 3]) {
            return Err(ControlServiceError::new(
                "Owner operation unavailable to guests",
            ));
        }
        self.inner.handle(id, context, payload)
    }
}
