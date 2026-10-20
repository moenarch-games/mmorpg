//! A process-local, expiring capability derived from a confirmed directory grant.
//! This cache never grants ownership, chooses another writer or performs I/O.
//! Its clock is independent of simulation ticks and its state is not replayed.
use crate::ZoneLease;
use mmorpg_core::ZoneId;
use std::{
    error::Error,
    fmt,
    sync::RwLock,
    time::{Duration, Instant},
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PermitError {
    InvalidLifetime,
    Expired,
    Revoked,
    WrongAuthority,
    StaleRenewal,
    Unavailable,
}
impl fmt::Display for PermitError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::InvalidLifetime => "invalid local authority permit lifetime",
            Self::Expired => "local zone authority permit expired",
            Self::Revoked => "local zone authority permit revoked",
            Self::WrongAuthority => "local permit belongs to a different zone authority",
            Self::StaleRenewal => "local permit renewal did not advance its deadline",
            Self::Unavailable => "local zone authority permit unavailable",
        })
    }
}
impl Error for PermitError {}

struct State {
    deadline: Instant,
    revoked: bool,
}

/// The backend must confirm the durable grant before constructing this value.
/// `valid_for` is a conservative remaining lease duration reported by the
/// authoritative directory. Anchor it at the *start* of the request that fetched
/// the grant, never at response receipt; network/storage delay cannot extend it.
/// The deployment must bound clock-rate error in its supplied duration. Database
/// write fencing and safe primary promotion remain independent requirements.
pub struct LocalZonePermit {
    lease: ZoneLease,
    state: RwLock<State>,
}
impl LocalZonePermit {
    pub fn from_confirmed_lease(
        lease: ZoneLease,
        request_started: Instant,
        valid_for: Duration,
    ) -> Result<Self, PermitError> {
        Self::confirmed_at(lease, request_started, valid_for, Instant::now())
    }
    fn confirmed_at(
        lease: ZoneLease,
        request_started: Instant,
        valid_for: Duration,
        now: Instant,
    ) -> Result<Self, PermitError> {
        if lease.epoch == 0 || valid_for.is_zero() || request_started > now {
            return Err(PermitError::InvalidLifetime);
        }
        let deadline = request_started
            .checked_add(valid_for)
            .ok_or(PermitError::InvalidLifetime)?;
        if now >= deadline {
            return Err(PermitError::Expired);
        }
        Ok(Self {
            lease,
            state: RwLock::new(State {
                deadline,
                revoked: false,
            }),
        })
    }
    /// Original grant identity. Renewals update the shared local deadline;
    /// callers must use `ensure_current`, never this copy's expiry field.
    #[must_use]
    pub fn lease(&self) -> &ZoneLease {
        &self.lease
    }

    pub fn ensure_current(&self, zone: ZoneId) -> Result<(), PermitError> {
        self.ensure_at(zone, Instant::now())
    }
    fn ensure_at(&self, zone: ZoneId, now: Instant) -> Result<(), PermitError> {
        if zone != self.lease.zone_id {
            return Err(PermitError::WrongAuthority);
        }
        let state = self.state.read().map_err(|_| PermitError::Unavailable)?;
        if state.revoked {
            return Err(PermitError::Revoked);
        }
        if now >= state.deadline {
            return Err(PermitError::Expired);
        }
        Ok(())
    }
    /// An expired permit is terminal even when a delayed renewal response arrives.
    /// The caller must stop/fence the old runtime and obtain a new valid grant.
    pub fn renew_confirmed(
        &self,
        lease: &ZoneLease,
        request_started: Instant,
        valid_for: Duration,
    ) -> Result<(), PermitError> {
        self.renew_at(lease, request_started, valid_for, Instant::now())
    }
    fn renew_at(
        &self,
        lease: &ZoneLease,
        request_started: Instant,
        valid_for: Duration,
        now: Instant,
    ) -> Result<(), PermitError> {
        if !self.lease.same_authority(lease) {
            return Err(PermitError::WrongAuthority);
        }
        if valid_for.is_zero() || request_started > now {
            return Err(PermitError::InvalidLifetime);
        }
        let deadline = request_started
            .checked_add(valid_for)
            .ok_or(PermitError::InvalidLifetime)?;
        let mut state = self.state.write().map_err(|_| PermitError::Unavailable)?;
        if state.revoked {
            return Err(PermitError::Revoked);
        }
        if now >= state.deadline || now >= deadline {
            return Err(PermitError::Expired);
        }
        if deadline <= state.deadline {
            return Err(PermitError::StaleRenewal);
        }
        state.deadline = deadline;
        Ok(())
    }
    pub fn revoke(&self) -> Result<(), PermitError> {
        self.state
            .write()
            .map_err(|_| PermitError::Unavailable)?
            .revoked = true;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::HostId;

    #[test]
    fn cached_permit_deducts_latency_and_never_resurrects_expired_authority() {
        let start = Instant::now();
        let ttl = Duration::from_secs(10);
        let lease = ZoneLease {
            zone_id: ZoneId::new(1),
            host_id: HostId::new("permit-test").unwrap(),
            epoch: 1,
            expires_at_tick: 100,
        };
        let permit = LocalZonePermit::confirmed_at(
            lease.clone(),
            start,
            ttl,
            start + Duration::from_secs(4),
        )
        .unwrap();
        assert_eq!(
            permit.ensure_at(lease.zone_id, start + ttl),
            Err(PermitError::Expired)
        );
        assert_eq!(
            permit.renew_at(&lease, start + ttl, ttl, start + ttl),
            Err(PermitError::Expired)
        );
        assert_eq!(
            permit.renew_at(
                &ZoneLease {
                    epoch: 2,
                    ..lease.clone()
                },
                start,
                ttl,
                start
            ),
            Err(PermitError::WrongAuthority)
        );
        // A separately live permit accepts a confirmed extension but cannot be
        // shortened by a delayed response or revived after explicit revocation.
        let live = LocalZonePermit::confirmed_at(lease.clone(), start, ttl, start).unwrap();
        live.renew_at(
            &lease,
            start + Duration::from_secs(5),
            ttl,
            start + Duration::from_secs(6),
        )
        .unwrap();
        live.ensure_at(lease.zone_id, start + Duration::from_secs(12))
            .unwrap();
        assert_eq!(
            live.renew_at(&lease, start, ttl, start + Duration::from_secs(6)),
            Err(PermitError::StaleRenewal)
        );
        live.revoke().unwrap();
        assert_eq!(
            live.ensure_at(lease.zone_id, start),
            Err(PermitError::Revoked)
        );
        assert_eq!(
            live.renew_at(&lease, start, ttl, start),
            Err(PermitError::Revoked)
        );
        assert!(matches!(
            LocalZonePermit::confirmed_at(lease, start, ttl, start + ttl),
            Err(PermitError::Expired)
        ));
    }
}
