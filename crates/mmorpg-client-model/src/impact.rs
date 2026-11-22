//! Which combat impacts a projection's confirmed events show (#52, #769, #1058): hits puff and
//! blocks ring on the struck unit. Only events the zone already delivered to this viewer, never
//! a predicted or inferred outcome; each client draws them its own way.

use mmorpg_core::{EntityRef, ZoneEvent, ZoneSnapshot};

/// What an impact looks like: a hit's puff or a block's ring.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum ImpactKind {
    Hit,
    Block,
}

/// One confirmed impact: the striking and the struck unit.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ImpactCue {
    pub kind: ImpactKind,
    pub source: EntityRef,
    pub target: EntityRef,
}

/// The impacts of `snapshot`'s events, in event order.
pub fn impact_cues(snapshot: &ZoneSnapshot) -> impl Iterator<Item = ImpactCue> + '_ {
    snapshot.events.iter().filter_map(|event| match *event {
        ZoneEvent::DamageDealt { source, target, .. }
        | ZoneEvent::DamageTaken { source, target, .. }
        | ZoneEvent::DamageObserved { source, target, .. } => Some(ImpactCue {
            kind: ImpactKind::Hit,
            source,
            target,
        }),
        ZoneEvent::Blocked { source, target, .. } => Some(ImpactCue {
            kind: ImpactKind::Block,
            source,
            target,
        }),
        _ => None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::snapshot;
    use mmorpg_core::{CreatureId, StrikeHeight, ZoneEvent};

    #[test]
    fn hits_puff_and_blocks_ring_on_the_struck_unit_in_event_order() {
        let me = EntityRef::Player(1);
        let wolf = EntityRef::Creature(CreatureId::new(7));
        let mut projection = snapshot(1, 5);
        projection.events = vec![
            ZoneEvent::DamageDealt {
                source: me,
                target: wolf,
                amount: 4,
                critical: false,
            },
            ZoneEvent::Miss {
                source: me,
                target: wolf,
            },
            ZoneEvent::Blocked {
                source: wolf,
                target: me,
                lane: StrikeHeight::High,
                amount: 0,
            },
        ];
        let cues: Vec<_> = impact_cues(&projection).collect();
        assert_eq!(
            cues,
            [
                ImpactCue {
                    kind: ImpactKind::Hit,
                    source: me,
                    target: wolf
                },
                ImpactCue {
                    kind: ImpactKind::Block,
                    source: wolf,
                    target: me
                },
            ]
        );
    }
}
