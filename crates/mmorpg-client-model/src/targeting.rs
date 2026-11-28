//! Tab targeting (#1280): which unit a press of Tab selects. Selection is only intent; the zone
//! validates every target.
use mmorpg_core::{EntityKind, EntityRef, ZoneSnapshot};

/// The nearest living attackable player or creature in `snapshot`; while one of them is
/// selected, the next one outward, wrapping round. Equal distances order by kind, then ID, so
/// repeated presses visit every candidate once. `None` without a candidate.
#[must_use]
pub fn next_tab_target(snapshot: &ZoneSnapshot) -> Option<EntityRef> {
    let viewer = snapshot.entities.first()?;
    let mut candidates: Vec<(i64, EntityRef)> = snapshot
        .entities
        .iter()
        .filter(|entity| {
            matches!(entity.kind, EntityKind::Creature | EntityKind::Player)
                && entity.flags.attackable
                && !entity.flags.dead
                && !(entity.kind == EntityKind::Player && entity.id == snapshot.viewer_id)
        })
        .map(|entity| {
            let dx = i64::from(entity.position[0]) - i64::from(viewer.position[0]);
            let dz = i64::from(entity.position[2]) - i64::from(viewer.position[2]);
            (dx * dx + dz * dz, entity.entity())
        })
        .collect();
    candidates.sort_unstable();
    let current = candidates
        .iter()
        .position(|(_, entity)| Some(*entity) == snapshot.viewer.target);
    let next = current.map_or(0, |index| (index + 1) % candidates.len());
    candidates.get(next).map(|(_, entity)| *entity)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::snapshot;
    use mmorpg_core::{EntityFlags, EntitySnapshot};

    fn unit(kind: EntityKind, id: u32, x: i32, attackable: bool, dead: bool) -> EntitySnapshot {
        EntitySnapshot {
            kind,
            id,
            appearance: 0,
            position: [x, 0, 0],
            velocity: [0; 3],
            facing: 0,
            level: 1,
            health_percent: 100,
            flags: EntityFlags {
                attackable,
                dead,
                ..EntityFlags::default()
            },
        }
    }

    #[test]
    fn tab_cycles_outward_over_living_players_and_creatures() {
        let mut view = snapshot(1, 5);
        view.entities = vec![
            unit(EntityKind::Player, 1, 0, false, false),
            unit(EntityKind::Creature, 9, 300, true, false),
            unit(EntityKind::Player, 4, 300, true, false),
            unit(EntityKind::Creature, 2, 100, true, false),
            unit(EntityKind::Creature, 3, 50, true, true),
            unit(EntityKind::Npc, 5, 10, false, false),
            unit(EntityKind::Creature, 6, 20, false, false),
        ];
        let creature = |id| EntityRef::new(EntityKind::Creature, id);
        let player = EntityRef::new(EntityKind::Player, 4);
        let mut seen = Vec::new();
        for _ in 0..4 {
            let next = next_tab_target(&view).unwrap();
            seen.push(next);
            view.viewer.target = Some(next);
        }
        // The dead creature and the unattackable units are skipped; at equal distance the
        // player (kind order) comes before the creature.
        assert_eq!(seen, [creature(2), player, creature(9), creature(2)]);
        view.entities.truncate(1);
        assert_eq!(next_tab_target(&view), None);
    }
}
