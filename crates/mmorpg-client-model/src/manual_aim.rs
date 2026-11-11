//! Temporary manual aim for a throw (#649, Atlas `5.2.i649-3`): which abilities take the
//! shoulder-camera aim, and where the throw is expected to land. Both are aids: the client sends
//! an ordinary ability use along the aimed facing, and the zone decides the throw's flight,
//! landing and every hit. The mouse owner ([`crate::mouse_owner`]) holds the aim's lifecycle.
use mmorpg_core::{
    AIM_ASSIST_COS, AbilityId, EntityRef, LAUNCH_HEIGHT_UNITS, PLAYER_HALF_EXTENTS_UNITS,
    ProjectileKind, THROW_RANGE_UNITS, THROW_RISE_UNITS, ZoneSnapshot, ability::ids, trig,
};

use crate::power_draw::can_draw;

/// The first opted-in throwable is the Rogue's Blasting Bomb; others follow once its throw feels
/// right (docs/CONTROLS.md, step 3).
#[must_use]
pub fn aims_manually(ability: u8) -> bool {
    AbilityId::new(ability) == ids::BLASTING_BOMB
}

/// A held press of `slot`, holding `ability`, starts a manual aim: the ability aims manually, is
/// learned and still on that slot, the viewer can act (alive, unstunned, not casting) and no
/// interface window is open. Otherwise the slot is used as usual.
#[must_use]
pub fn can_aim(snapshot: &ZoneSnapshot, slot: u8, ability: u8, interface_open: bool) -> bool {
    aims_manually(ability) && !interface_open && can_draw(snapshot, slot, ability)
}

/// Whether an aim on `slot` holding `ability` is still possible in this projection.
#[must_use]
pub fn still_aims(snapshot: &ZoneSnapshot, slot: u8, ability: u8) -> bool {
    can_draw(snapshot, slot, ability)
}

/// Where a throw along `facing` is expected to land, in core units: on the nearest attackable
/// unit straight ahead within the throw's reach, as the zone arcs it, otherwise on level ground
/// the nominal lob away. A non-authoritative aid; terrain, walls and bounces are the zone's.
#[must_use]
pub fn landing(snapshot: &ZoneSnapshot, facing: u16) -> Option<[i32; 3]> {
    let viewer = EntityRef::Player(snapshot.viewer_id);
    let me = snapshot
        .entities
        .iter()
        .find(|entity| entity.entity() == viewer)?;
    let (x, z) = trig::direction(facing);
    #[allow(clippy::cast_precision_loss)]
    let ahead = [
        x as f32 / trig::TRIG_ONE as f32,
        z as f32 / trig::TRIG_ONE as f32,
    ];
    #[allow(clippy::cast_precision_loss)]
    let range = THROW_RANGE_UNITS as f32;
    let target = snapshot
        .entities
        .iter()
        .filter(|entity| entity.entity() != viewer && entity.flags.attackable && !entity.flags.dead)
        .filter_map(|entity| {
            #[allow(clippy::cast_precision_loss)]
            let delta = [
                (entity.position[0] - me.position[0]) as f32,
                (entity.position[2] - me.position[2]) as f32,
            ];
            let distance = delta[0].hypot(delta[1]);
            let along = (delta[0] * ahead[0] + delta[1] * ahead[1]) / distance;
            (distance > 0.0 && distance <= range && along >= AIM_ASSIST_COS)
                .then_some((distance, entity.position))
        })
        .min_by(|a, b| a.0.total_cmp(&b.0));
    if let Some((_, position)) = target {
        return Some(position);
    }
    // Level ground: the lob leaves chest-high at the throw's speed and rise and falls under its
    // gravity to the viewer's feet.
    let gravity = ProjectileKind::Bomb.gravity();
    #[allow(clippy::cast_precision_loss)]
    let height = LAUNCH_HEIGHT_UNITS + PLAYER_HALF_EXTENTS_UNITS[1] as f32;
    let ticks = (THROW_RISE_UNITS
        + THROW_RISE_UNITS
            .mul_add(THROW_RISE_UNITS, 2.0 * gravity * height)
            .sqrt())
        / gravity;
    let distance = ProjectileKind::Bomb.speed() * ticks;
    #[allow(clippy::cast_possible_truncation)]
    Some([
        me.position[0] + (ahead[0] * distance).round() as i32,
        me.position[1] - PLAYER_HALF_EXTENTS_UNITS[1],
        me.position[2] + (ahead[1] * distance).round() as i32,
    ])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::snapshot;
    use mmorpg_core::{ClassChoice, EntityFlags, EntityKind, EntitySnapshot, PlayerClass, Sex};

    const BOMB: u8 = ids::BLASTING_BOMB.get();

    fn unit(kind: EntityKind, id: u32, position: [i32; 3], attackable: bool) -> EntitySnapshot {
        EntitySnapshot {
            kind,
            id,
            appearance: 0,
            position,
            velocity: [0; 3],
            facing: 0,
            level: 6,
            health_percent: 100,
            flags: EntityFlags {
                attackable,
                ..EntityFlags::default()
            },
        }
    }

    fn rogue() -> ZoneSnapshot {
        let mut snapshot = snapshot(1, 1);
        snapshot.viewer.level = 10;
        snapshot.viewer.class = Some(ClassChoice {
            class: PlayerClass::Rogue,
            sex: Sex::Female,
        });
        snapshot
            .entities
            .push(unit(EntityKind::Player, 1, [0, 90, 0], false));
        snapshot
    }

    fn bomb_slot() -> u8 {
        let kit = crate::hud::class_kit(PlayerClass::Rogue);
        u8::try_from(kit.iter().position(|&id| id == BOMB).unwrap() + 1).unwrap()
    }

    #[test]
    fn only_the_bomb_aims_and_only_from_its_slot() {
        let snapshot = rogue();
        let slot = bomb_slot();
        assert!(can_aim(&snapshot, slot, BOMB, false));
        assert!(!can_aim(&snapshot, slot, BOMB, true), "an open window");
        assert!(!can_aim(&snapshot, slot + 1, BOMB, false), "the wrong slot");
        assert!(!aims_manually(ids::VENOM_FLASK.get()));
        assert!(!aims_manually(ids::THROWING_KNIFE.get()));
    }

    #[test]
    fn the_landing_lobs_twelve_metres_or_onto_a_unit_straight_ahead() {
        let mut snapshot = rogue();
        // Facing +Z (yaw 0): about 12 m ahead on the viewer's ground.
        let [x, y, z] = landing(&snapshot, 0).unwrap();
        assert_eq!((x, y), (0, 0));
        assert!((1_150..=1_250).contains(&z), "{z}");
        // A wolf 8 m ahead takes the throw; one off to the side does not.
        snapshot
            .entities
            .push(unit(EntityKind::Creature, 7, [0, 40, 800], true));
        snapshot
            .entities
            .push(unit(EntityKind::Creature, 8, [500, 40, 500], true));
        assert_eq!(landing(&snapshot, 0), Some([0, 40, 800]));
        // Turned a quarter toward +X, neither is ahead.
        let [x, _, z] = landing(&snapshot, trig::YAW_QUARTER_TURN).unwrap();
        assert!(x > 1_000 && z.abs() < 5, "{x} {z}");
    }
}
