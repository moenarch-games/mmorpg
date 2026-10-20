//! Immutable compressed checkpoint objects (TECH-003). These functions perform
//! storage I/O outside the simulation. Only upload + bounded readback verification
//! constructs a reference that the journal can publish.
//!
//! S3 acceptance provider: RustFS 1.0.3, image
//! `rustfs/rustfs:1.0.3@sha256:9ca9559533b537d10482fe923288b676ad08f92a7069d9a17327fbae3cab688b`.
//! Source/release: <https://github.com/rustfs/rustfs/releases/tag/1.0.3>.
//! Required APIs are conditional PUT (`If-None-Match: *`) and streaming GET;
//! replacing the provider must preserve those semantics. Object keys are content
//! addressed and the host never requests an overwrite or deletes old objects.
use super::postgres::{CheckpointReference, StreamFence};
use futures_util::TryStreamExt;
use game_server::{MAX_RECOVERY_IMAGE_BYTES, RecoveryImage};
use object_store::{
    ClientOptions, ObjectStore, ObjectStoreExt, PutMode, RetryConfig,
    aws::{AmazonS3Builder, AmazonS3ConfigKey, S3ConditionalPut},
    path::Path,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{fmt, io::Read, sync::Arc, time::Duration};

const MAGIC: &[u8; 5] = b"MMOZ\x01";
const MAX_CONTEXT_BYTES: usize = 4096;

#[derive(Debug)]
pub(crate) enum CheckpointError {
    Unavailable(String),
    Invalid(String),
}
impl fmt::Display for CheckpointError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unavailable(message) => write!(f, "checkpoint storage unavailable: {message}"),
            Self::Invalid(message) => write!(f, "invalid checkpoint: {message}"),
        }
    }
}
impl std::error::Error for CheckpointError {}

#[derive(Clone, Copy)]
pub(crate) struct CheckpointLimits {
    pub max_compressed_bytes: usize,
    pub max_decoded_bytes: usize,
    pub timeout: Duration,
}
impl CheckpointLimits {
    fn validate(self) -> Result<Self, CheckpointError> {
        if self.max_compressed_bytes == 0
            || self.max_decoded_bytes == 0
            || self.max_decoded_bytes > MAX_RECOVERY_IMAGE_BYTES
            || self.max_compressed_bytes > MAX_RECOVERY_IMAGE_BYTES
            || self.timeout.is_zero()
        {
            return Err(invalid("invalid checkpoint byte/time limits"));
        }
        Ok(self)
    }
}

#[derive(Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Context {
    zone_id: u32,
    world_id: String,
    timeline: u64,
    position: u64,
    tick: u64,
    content_fingerprint: u64,
    canonical_version: u16,
}
impl Context {
    fn from_reference(reference: &CheckpointReference) -> Self {
        Self {
            zone_id: reference.zone_id,
            world_id: reference.world_id.clone(),
            timeline: reference.timeline,
            position: reference.position,
            tick: reference.tick,
            content_fingerprint: reference.content_fingerprint,
            canonical_version: reference.canonical_version,
        }
    }
}

pub(crate) struct PreparedCheckpoint {
    reference: CheckpointReference,
    compressed: Vec<u8>,
}
pub(crate) struct VerifiedCheckpoint {
    reference: CheckpointReference,
}

pub(crate) struct PreparedWorldReset {
    checkpoint: PreparedCheckpoint,
}
pub(crate) struct VerifiedWorldReset {
    checkpoint: VerifiedCheckpoint,
}
impl VerifiedWorldReset {
    pub(super) fn checkpoint(&self) -> &VerifiedCheckpoint {
        &self.checkpoint
    }
    #[cfg(test)]
    pub(super) fn for_sql_test(reference: CheckpointReference) -> Self {
        Self {
            checkpoint: VerifiedCheckpoint::for_sql_test(reference),
        }
    }
}

impl VerifiedCheckpoint {
    pub(crate) fn reference(&self) -> &CheckpointReference {
        &self.reference
    }
    // SQL composition tests deliberately replace object storage with a fixture.
    #[cfg(test)]
    pub(super) fn for_sql_test(reference: CheckpointReference) -> Self {
        Self { reference }
    }
}

pub(crate) struct S3Config {
    pub endpoint: String,
    pub bucket: String,
    pub region: String,
    pub allow_http: bool,
}

pub(crate) struct CheckpointObjects {
    store: Arc<dyn ObjectStore>,
    limits: CheckpointLimits,
}
impl CheckpointObjects {
    /// Credentials follow the SDK's environment/provider chain; the destination
    /// and path-style/conditional-write contract are explicit host configuration.
    pub(crate) fn s3(config: S3Config, limits: CheckpointLimits) -> Result<Self, CheckpointError> {
        let limits = limits.validate()?;
        let store = AmazonS3Builder::from_env()
            .with_config(AmazonS3ConfigKey::S3Endpoint, config.endpoint.clone())
            .with_endpoint(config.endpoint)
            .with_bucket_name(config.bucket)
            .with_region(config.region)
            .with_virtual_hosted_style_request(false)
            .with_conditional_put(S3ConditionalPut::ETagMatch)
            .with_client_options(
                ClientOptions::new()
                    .with_timeout(limits.timeout)
                    .with_connect_timeout(limits.timeout),
            )
            .with_allow_http(config.allow_http)
            .with_retry(RetryConfig {
                max_retries: 2,
                retry_timeout: limits.timeout,
                ..RetryConfig::default()
            })
            .build()
            .map_err(invalid)?;
        Self::new(Arc::new(store), limits)
    }

    pub(crate) fn new(
        store: Arc<dyn ObjectStore>,
        limits: CheckpointLimits,
    ) -> Result<Self, CheckpointError> {
        Ok(Self {
            store,
            limits: limits.validate()?,
        })
    }

    /// Explicit reset preparation, never a fallback for failed recovery. The
    /// source writer must be retired separately before publishing this object.
    /// Reuse the same world UUID so already committed grants keep their identity.
    pub(crate) fn prepare_world_reset(
        &self,
        previous: &StreamFence,
        content: Arc<mmorpg_core::ZoneContent>,
        image: &RecoveryImage,
    ) -> Result<PreparedWorldReset, CheckpointError> {
        if !image.sessions.sessions.is_empty() {
            return Err(invalid("world reset checkpoint must contain no sessions"));
        }
        let restored = game_server::MatchRuntime::restore_from_recovery(
            crate::ZoneGameServerAdapter::with_content(
                previous.lease.zone_id,
                Arc::clone(&content),
            )
            .map_err(invalid)?,
            image.clone(),
        )
        .map_err(invalid)?;
        let snapshot = restored.snapshot().map_err(invalid)?;
        let (zone, characters) = crate::ZoneGameServerAdapter::decode_stored_zone(
            &snapshot.payload,
            Arc::clone(&content),
        )
        .map_err(invalid)?;
        if !characters.is_empty()
            || !zone.snapshot().map_err(invalid)?.players.is_empty()
            || zone.reward_history().is_none()
        {
            return Err(invalid(
                "world reset requires an empty world with reward tracking",
            ));
        }
        let next = StreamFence {
            timeline: previous
                .timeline
                .checked_add(1)
                .ok_or_else(|| invalid("world timelines exhausted"))?,
            ..previous.clone()
        };
        let checkpoint = self.prepare(
            &next,
            0,
            content.fingerprint(),
            u16::from(mmorpg_protocol::SNAPSHOT_WIRE_VERSION),
            image,
        )?;
        Ok(PreparedWorldReset { checkpoint })
    }
    pub(crate) async fn upload_world_reset(
        &self,
        prepared: &PreparedWorldReset,
    ) -> Result<VerifiedWorldReset, CheckpointError> {
        Ok(VerifiedWorldReset {
            checkpoint: self.upload(&prepared.checkpoint).await?,
        })
    }

    /// CPU-only capture/encoding. The host calls this on its persistence worker,
    /// after an immutable tick-boundary image has left the simulation.
    pub(crate) fn prepare(
        &self,
        fence: &StreamFence,
        position: u64,
        content_fingerprint: u64,
        canonical_version: u16,
        image: &RecoveryImage,
    ) -> Result<PreparedCheckpoint, CheckpointError> {
        let mut reference = CheckpointReference {
            version: 1,
            zone_id: fence.lease.zone_id.get(),
            world_id: fence.world_id.clone(),
            timeline: fence.timeline,
            position,
            tick: image.current_tick,
            object_key: String::new(),
            sha256: [0; 32],
            byte_length: 0,
            content_fingerprint,
            canonical_version,
        };
        let context = serde_json::to_vec(&Context::from_reference(&reference)).map_err(invalid)?;
        if context.len() > MAX_CONTEXT_BYTES {
            return Err(invalid("checkpoint context too large"));
        }
        let image = image.encode().map_err(invalid)?;
        let length = MAGIC.len() + 4 + context.len() + image.len();
        if length > self.limits.max_decoded_bytes {
            return Err(invalid("checkpoint decoded byte limit exceeded"));
        }
        let mut encoded = Vec::with_capacity(length);
        encoded.extend_from_slice(MAGIC);
        encoded.extend_from_slice(&(context.len() as u32).to_be_bytes());
        encoded.extend_from_slice(&context);
        encoded.extend_from_slice(&image);
        let compressed = zstd::stream::encode_all(encoded.as_slice(), 3).map_err(invalid)?;
        if compressed.len() > self.limits.max_compressed_bytes {
            return Err(invalid("checkpoint compressed byte limit exceeded"));
        }
        reference.sha256 = Sha256::digest(&compressed).into();
        reference.byte_length = compressed.len() as u64;
        reference.object_key = object_key(&reference);
        reference.validate(fence).map_err(invalid)?;
        Ok(PreparedCheckpoint {
            reference,
            compressed,
        })
    }

    pub(crate) async fn upload(
        &self,
        prepared: &PreparedCheckpoint,
    ) -> Result<VerifiedCheckpoint, CheckpointError> {
        if prepared.compressed.len() > self.limits.max_compressed_bytes {
            return Err(invalid(
                "prepared checkpoint exceeds this store's byte limit",
            ));
        }
        tokio::time::timeout(self.limits.timeout, async {
            let key = Path::parse(&prepared.reference.object_key).map_err(invalid)?;
            match self
                .store
                .put_opts(
                    &key,
                    prepared.compressed.clone().into(),
                    PutMode::Create.into(),
                )
                .await
            {
                Ok(_)
                | Err(
                    object_store::Error::AlreadyExists { .. }
                    | object_store::Error::Precondition { .. },
                ) => {}
                Err(error) => return Err(unavailable(error)),
            }
            // A successful PUT or an existing key alone proves neither content
            // nor readability. Verify actual bytes before exposing the reference.
            self.read_compressed(&prepared.reference).await?;
            Ok(VerifiedCheckpoint {
                reference: prepared.reference.clone(),
            })
        })
        .await
        .map_err(|_| unavailable("upload/readback timeout"))?
    }

    pub(crate) async fn load(
        &self,
        fence: &StreamFence,
        reference: &CheckpointReference,
        content_fingerprint: u64,
        canonical_version: u16,
    ) -> Result<RecoveryImage, CheckpointError> {
        reference.validate(fence).map_err(invalid)?;
        if reference.object_key != object_key(reference)
            || reference.content_fingerprint != content_fingerprint
            || reference.canonical_version != canonical_version
        {
            return Err(invalid(
                "checkpoint content/schema/object identity is incompatible",
            ));
        }
        let compressed = tokio::time::timeout(self.limits.timeout, self.read_compressed(reference))
            .await
            .map_err(|_| unavailable("checkpoint read timeout"))??;
        // Limit output independently of compressed length: a valid digest does
        // not make a compressed payload safe to allocate without a decoded cap.
        let decoder = zstd::stream::read::Decoder::new(compressed.as_slice()).map_err(invalid)?;
        let mut decoded = Vec::new();
        decoder
            .take(self.limits.max_decoded_bytes as u64 + 1)
            .read_to_end(&mut decoded)
            .map_err(invalid)?;
        if decoded.len() > self.limits.max_decoded_bytes || decoded.get(..5) != Some(MAGIC) {
            return Err(invalid("checkpoint decoded limit or format mismatch"));
        }
        let length = u32::from_be_bytes(
            decoded
                .get(5..9)
                .ok_or_else(|| invalid("truncated checkpoint"))?
                .try_into()
                .map_err(invalid)?,
        ) as usize;
        if length > MAX_CONTEXT_BYTES {
            return Err(invalid("checkpoint context too large"));
        }
        let context: Context = serde_json::from_slice(
            decoded
                .get(9..9 + length)
                .ok_or_else(|| invalid("truncated checkpoint context"))?,
        )
        .map_err(invalid)?;
        if context != Context::from_reference(reference) {
            return Err(invalid("checkpoint object context differs from reference"));
        }
        let image = RecoveryImage::decode(&decoded[9 + length..]).map_err(invalid)?;
        if image.current_tick != reference.tick {
            return Err(invalid("checkpoint tick differs from reference"));
        }
        Ok(image)
    }

    async fn read_compressed(
        &self,
        reference: &CheckpointReference,
    ) -> Result<Vec<u8>, CheckpointError> {
        let expected: usize = reference.byte_length.try_into().map_err(invalid)?;
        if expected == 0 || expected > self.limits.max_compressed_bytes {
            return Err(invalid("checkpoint object length exceeds limit"));
        }
        let key = Path::parse(&reference.object_key).map_err(invalid)?;
        let result = self.store.get(&key).await.map_err(unavailable)?;
        if result.meta.size != reference.byte_length {
            return Err(invalid("checkpoint object metadata length mismatch"));
        }
        let mut stream = result.into_stream();
        let mut bytes = Vec::with_capacity(expected);
        while let Some(chunk) = stream.try_next().await.map_err(unavailable)? {
            if chunk.len() > expected.saturating_sub(bytes.len()) {
                return Err(invalid("checkpoint object exceeds declared length"));
            }
            bytes.extend_from_slice(&chunk);
        }
        if bytes.len() != expected || Sha256::digest(&bytes).as_slice() != reference.sha256 {
            return Err(invalid("checkpoint object digest/length mismatch"));
        }
        Ok(bytes)
    }
}
fn object_key(reference: &CheckpointReference) -> String {
    let digest: String = reference
        .sha256
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    format!(
        "checkpoints/{}/{}/{}/{}-{}-{digest}.zst",
        reference.zone_id,
        reference.world_id,
        reference.timeline,
        reference.position,
        reference.tick
    )
}
fn invalid(message: impl fmt::Display) -> CheckpointError {
    CheckpointError::Invalid(message.to_string())
}
fn unavailable(message: impl fmt::Display) -> CheckpointError {
    CheckpointError::Unavailable(message.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ZoneGameServerAdapter;
    use game_server::MatchRuntime;
    use mmorpg_control_plane::{HostId, ZoneLease};
    use mmorpg_core::{ZoneId, greyhaven_vale};

    #[tokio::test]
    async fn rustfs_checkpoint_round_trip_is_immutable_bounded_and_context_checked() {
        let Ok(endpoint) = std::env::var("MMORPG_CHECKPOINT_S3_ENDPOINT") else {
            return;
        };
        let limits = CheckpointLimits {
            max_compressed_bytes: 1024 * 1024,
            max_decoded_bytes: 4 * 1024 * 1024,
            timeout: Duration::from_secs(5),
        };
        let objects = CheckpointObjects::s3(
            S3Config {
                endpoint,
                bucket: std::env::var("MMORPG_CHECKPOINT_S3_BUCKET").unwrap(),
                region: "us-east-1".into(),
                allow_http: true,
            },
            limits,
        )
        .unwrap();
        let zone = ZoneId::new(42);
        let fence = StreamFence {
            lease: ZoneLease {
                zone_id: zone,
                host_id: HostId::new("checkpoint-test").unwrap(),
                epoch: 1,
                expires_at_tick: 1000,
            },
            world_id: "11111111-1111-1111-1111-111111111111".into(),
            timeline: 1,
        };
        let adapter =
            || ZoneGameServerAdapter::with_content(zone, greyhaven_vale::content()).unwrap();
        let mut runtime = MatchRuntime::new_with_replay_capture(adapter(), 30);
        runtime.advance_tick().unwrap();
        let snapshot = runtime.snapshot().unwrap();
        let image = runtime.checkpoint_replay().unwrap();
        let fingerprint = greyhaven_vale::content().fingerprint();
        let version = u16::from(mmorpg_protocol::SNAPSHOT_WIRE_VERSION);
        let prepared = objects
            .prepare(&fence, 1, fingerprint, version, &image)
            .unwrap();
        let verified = objects.upload(&prepared).await.unwrap();
        assert_eq!(
            objects.upload(&prepared).await.unwrap().reference(),
            verified.reference()
        );
        // The provider, not a caller convention, must reject overwriting an
        // existing object even when supplied different content.
        let key = Path::parse(&verified.reference().object_key).unwrap();
        assert!(matches!(
            objects
                .store
                .put_opts(&key, b"wrong".to_vec().into(), PutMode::Create.into())
                .await,
            Err(object_store::Error::AlreadyExists { .. }
                | object_store::Error::Precondition { .. })
        ));
        let restored_image = objects
            .load(&fence, verified.reference(), fingerprint, version)
            .await
            .unwrap();
        assert_eq!(restored_image, image);
        let restored = MatchRuntime::restore_from_recovery(adapter(), restored_image).unwrap();
        assert_eq!(restored.snapshot().unwrap(), snapshot);
        assert!(
            objects
                .load(&fence, verified.reference(), fingerprint ^ 1, version)
                .await
                .is_err()
        );
        let wrong_tick = CheckpointReference {
            tick: 2,
            ..verified.reference().clone()
        };
        assert!(
            objects
                .load(&fence, &wrong_tick, fingerprint, version)
                .await
                .is_err()
        );
        let small = CheckpointObjects::new(
            Arc::clone(&objects.store),
            CheckpointLimits {
                max_decoded_bytes: 32,
                ..limits
            },
        )
        .unwrap();
        assert!(
            small
                .load(&fence, verified.reference(), fingerprint, version)
                .await
                .is_err()
        );
        let mut corrupt = prepared.compressed.clone();
        corrupt[0] ^= 1;
        // Bypass conditional PUT as an operator-corruption fixture, then restore
        // the bytes so repeated invocations still own a valid immutable object.
        objects.store.put(&key, corrupt.into()).await.unwrap();
        let rejected = objects
            .load(&fence, verified.reference(), fingerprint, version)
            .await;
        objects
            .store
            .put(&key, prepared.compressed.into())
            .await
            .unwrap();
        assert!(matches!(rejected, Err(CheckpointError::Invalid(_))));
        let mut initial = adapter();
        initial.zone_mut().enable_reward_tracking(64, 16).unwrap();
        let mut initial = MatchRuntime::new_with_replay_capture(initial, 30);
        let reset_image = initial.checkpoint_replay().unwrap();
        let reset = objects
            .prepare_world_reset(&fence, greyhaven_vale::content(), &reset_image)
            .unwrap();
        let verified = objects.upload_world_reset(&reset).await.unwrap();
        let next = StreamFence {
            timeline: fence.timeline + 1,
            ..fence.clone()
        };
        assert_eq!(verified.checkpoint().reference().world_id, fence.world_id);
        assert_eq!(verified.checkpoint().reference().position, 0);
        assert_eq!(
            objects
                .load(
                    &next,
                    verified.checkpoint().reference(),
                    fingerprint,
                    version
                )
                .await
                .unwrap(),
            reset_image
        );
        initial.admit(game_server::ReconnectToken([9; 16])).unwrap();
        let occupied = initial.checkpoint_replay().unwrap();
        assert!(
            objects
                .prepare_world_reset(&fence, greyhaven_vale::content(), &occupied)
                .is_err()
        );
    }
}
