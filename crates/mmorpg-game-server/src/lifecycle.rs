//! Host-owned empty-zone policy and explicit deterministic offline transition seam.
//! No subsystem opts into offline progression merely because a zone was unloaded.
use std::time::Duration;

/// Advisory semantic input from the zone/content owner, never permission to advance it.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SuspensionHints {
    pub expected_return_after: Option<Duration>,
}

/// Zone-supplied bounds; the host chooses a proposal from runtime conditions.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SuspensionPolicy {
    minimum: Duration,
    base: Duration,
    maximum: Duration,
    pub hints: SuspensionHints,
}
impl SuspensionPolicy {
    pub fn new(minimum: Duration, base: Duration, maximum: Duration) -> Option<Self> {
        (minimum <= base && base <= maximum).then_some(Self {
            minimum,
            base,
            maximum,
            hints: SuspensionHints::default(),
        })
    }

    /// No proposal means no adaptive adjustment. All host proposals remain bounded.
    #[must_use]
    pub fn effective_grace(self, proposed: Option<Duration>) -> Duration {
        proposed
            .unwrap_or(self.base)
            .clamp(self.minimum, self.maximum)
    }
}

/// Tracks continuously empty residence using explicit monotonic host time.
#[derive(Clone, Debug, Default)]
pub struct EmptyGrace {
    since: Option<Duration>,
}
impl EmptyGrace {
    /// Reconnectable slots and active players both count as occupied.
    pub fn due(&mut self, now: Duration, occupied_slots: usize, grace: Duration) -> bool {
        if occupied_slots != 0 {
            self.since = None;
            return false;
        }
        let since = *self.since.get_or_insert(now);
        now.saturating_sub(since) >= grace
    }
}

/// Measured by the lifecycle layer and supplied explicitly, never read by core.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ResumeInput {
    pub elapsed_real_millis: u64,
}
impl ResumeInput {
    /// A subsystem chooses its own bounded transition horizon, not skipped ticks.
    #[must_use]
    pub fn bounded_millis(self, maximum: u64) -> u64 {
        self.elapsed_real_millis.min(maximum)
    }
}

/// An explicit opt-in for one subsystem, implemented by its gameplay owner.
/// It must be deterministic, bounded and transactional: return a new checkpoint
/// from the same checkpoint/input without external I/O or skipped-tick replay.
/// No production Greyhaven subsystem opts in through this foundation slice.
pub trait OfflineTransition {
    type Checkpoint;
    type Error;
    fn resume(
        &self,
        checkpoint: &Self::Checkpoint,
        input: ResumeInput,
    ) -> Result<Self::Checkpoint, Self::Error>;
}

/// Default zone resume preserves the exact authoritative recovery evidence.
#[must_use]
pub fn frozen_resume<T>(checkpoint: T, _input: ResumeInput) -> T {
    checkpoint
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn grace_defaults_are_predictable_and_host_adaptation_is_bounded() {
        let policy = SuspensionPolicy::new(
            Duration::from_secs(5),
            Duration::from_secs(30),
            Duration::from_secs(300),
        )
        .unwrap();
        assert_eq!(policy.effective_grace(None), Duration::from_secs(30));
        assert_eq!(
            policy.effective_grace(Some(Duration::ZERO)),
            Duration::from_secs(5)
        );
        assert_eq!(
            policy.effective_grace(Some(Duration::MAX)),
            Duration::from_secs(300)
        );
        let mut hinted = policy;
        hinted.hints.expected_return_after = Some(Duration::from_secs(60));
        assert_eq!(hinted.effective_grace(None), Duration::from_secs(30));
        assert_eq!(
            hinted.effective_grace(hinted.hints.expected_return_after),
            Duration::from_secs(60)
        );
        let mut empty = EmptyGrace::default();
        assert!(!empty.due(Duration::ZERO, 0, policy.effective_grace(None)));
        assert!(!empty.due(Duration::from_secs(29), 0, policy.effective_grace(None)));
        assert!(!empty.due(Duration::from_secs(30), 1, policy.effective_grace(None)));
        assert!(!empty.due(Duration::from_secs(31), 0, policy.effective_grace(None)));
        assert!(empty.due(Duration::from_secs(61), 0, policy.effective_grace(None)));
    }

    #[test]
    fn offline_progress_is_explicit_bounded_and_replayable_while_default_resume_freezes() {
        struct TestSubsystem;
        impl OfflineTransition for TestSubsystem {
            type Checkpoint = u64;
            type Error = std::convert::Infallible;
            fn resume(&self, checkpoint: &u64, input: ResumeInput) -> Result<u64, Self::Error> {
                Ok(checkpoint.saturating_add(input.bounded_millis(60_000) / 1_000))
            }
        }
        let checkpoint = 17;
        let input = ResumeInput {
            elapsed_real_millis: u64::MAX,
        };
        assert_eq!(frozen_resume(checkpoint, input), checkpoint);
        let first = TestSubsystem.resume(&checkpoint, input).unwrap();
        assert_eq!(first, 77);
        assert_eq!(TestSubsystem.resume(&checkpoint, input).unwrap(), first);
        assert_eq!(
            TestSubsystem
                .resume(
                    &checkpoint,
                    ResumeInput {
                        elapsed_real_millis: 0
                    }
                )
                .unwrap(),
            checkpoint
        );
    }
}
