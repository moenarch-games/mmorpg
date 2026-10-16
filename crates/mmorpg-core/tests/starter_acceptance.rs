//! Starter-zone acceptance (#24): on the hosted Greyhaven Vale content, a new
//! Warden fights Field Marauders through ordinary commands until it reaches
//! level 2 and a marauder drops gear, claims that drop and equips it. Every
//! outcome comes from the real templates, loot tables and zone RNG.
use std::sync::Arc;

use mmorpg_core::ability::ids;
use mmorpg_core::{
    EntityKind, ItemId, ZoneCommand, ZoneId, ZoneSimulation, greyhaven_vale, item_template,
};

/// Ten seconds of ticks per fight; far more than any marauder needs.
const MAX_TICKS: u64 = 30 * 60 * 20;
/// Among the grain rats south of Millbrook's barn, out of the marauders' aggro.
const START: [i32; 3] = [8_000, 90, 3_000];
/// Rats north of this line forage within the marauders' aggro radius.
const RAT_LINE_Z: i32 = 4_600;
const GRAIN_RAT: u16 = 3;
const FIELD_MARAUDER: u16 = 4;

struct Script {
    zone: ZoneSimulation,
    sequence: u32,
    walking: bool,
    facing: u16,
}

impl Script {
    /// Runs toward `goal` until within melee and loot reach, then stands.
    fn approach(&mut self, me: [i32; 3], goal: [i32; 3]) {
        let [dx, dz] = [f64::from(goal[0] - me[0]), f64::from(goal[2] - me[2])];
        let far = dx.hypot(dz) > 180.0;
        if far != self.walking || far {
            let facing = ((dx.atan2(dz) / std::f64::consts::TAU).rem_euclid(1.0) * 65_536.0) as u16;
            if far != self.walking || self.facing.abs_diff(facing) > 1_024 {
                self.walking = far;
                self.facing = facing;
                self.send(ZoneCommand::Move {
                    forward: i8::from(far),
                    strafe: 0,
                    facing,
                });
            }
        }
    }

    fn send(&mut self, command: ZoneCommand) {
        self.sequence += 1;
        self.zone.apply_command(1, self.sequence, command).unwrap();
    }
}

/// The last bag slot holding `item`: a claimed drop lands after the starter
/// Worn Dagger in slot 1.
fn bag_slot(view: &mmorpg_core::ZoneSnapshot, item: ItemId) -> Option<u8> {
    let bag = view.inventory.as_ref()?;
    let slot = bag
        .slots()
        .iter()
        .rposition(|stack| stack.is_some_and(|stack| stack.item() == item))?;
    u8::try_from(slot).ok().filter(|&slot| slot > 1)
}

#[test]
fn a_new_warden_reaches_level_two_and_equips_a_marauder_drop() {
    let mut zone = ZoneSimulation::with_content(ZoneId::new(1), greyhaven_vale::content()).unwrap();
    zone.add_player(1).unwrap();
    // Setup only: the walk from the hub is covered by `vale-wolf-hunt`.
    let mut state = zone.snapshot().unwrap();
    state.players[0].position = START;
    let zone = ZoneSimulation::from_snapshot(state, Arc::clone(zone.content())).unwrap();
    let mut script = Script {
        zone,
        sequence: 0,
        walking: false,
        facing: 0,
    };
    script.send(ZoneCommand::ChooseClass { class: 0, sex: 1 });

    let drop;
    let mut claimed_gear = None;
    let mut ticks = 0;
    loop {
        script.zone.advance_tick().unwrap();
        ticks += 1;
        assert!(
            ticks < MAX_TICKS,
            "no level-2 drop within {MAX_TICKS} ticks"
        );
        let view = script.zone.snapshot_for_player(1).unwrap();
        assert!(!view.viewer.dead, "the Warden died at tick {}", view.tick);
        if view.viewer.level >= 2
            && let Some(item) = claimed_gear
            && let Some(slot) = bag_slot(&view, item)
        {
            drop = (slot, item);
            break;
        }
        if let Some(loot) = view.loot {
            // Only a marauder's gear counts as the drop to equip.
            if let Some(stack) = loot.rewards.item
                && item_template(stack.item()).is_some_and(|item| item.slot.is_some())
            {
                claimed_gear = Some(stack.item());
            }
            script.send(ZoneCommand::Loot(loot.claim));
            continue;
        }
        let me = view.entities[0].position;
        let target = view
            .entities
            .iter()
            .find(|unit| Some(unit.entity()) == view.viewer.target);
        // A living target is fought in melee; an owned corpse is walked up to
        // until its loot sheet is projected and claimed above.
        if let Some(unit) = target.filter(|unit| !unit.flags.dead || unit.flags.lootable) {
            script.approach(me, unit.position);
            if !unit.flags.dead {
                if !view.viewer.auto_attacking {
                    script.send(ZoneCommand::StartAttack);
                } else if view.viewer.resource.is_some_and(|rage| rage.value >= 15) {
                    script.send(ZoneCommand::UseAbility {
                        ability: ids::HEROIC_STRIKE.get(),
                        target: None,
                    });
                }
            }
            continue;
        }
        script.approach(me, me);
        // Answer an attacker first; otherwise rest to full health, then pull
        // the nearest grain rat until level 2 and the nearest Field Marauder,
        // whose drops include gear, after that.
        let attacked = view.entities.iter().any(|unit| unit.flags.targets_viewer);
        if !attacked && view.viewer.health < view.viewer.max_health {
            continue;
        }
        let prey = if view.viewer.level < 2 {
            GRAIN_RAT
        } else {
            FIELD_MARAUDER
        };
        let next = view
            .entities
            .iter()
            .filter(|unit| {
                unit.kind == EntityKind::Creature && !unit.flags.dead && unit.flags.attackable
            })
            .filter(|unit| {
                unit.flags.targets_viewer
                    || (unit.appearance == prey
                        && (prey != GRAIN_RAT || unit.position[2] < RAT_LINE_Z))
            })
            .min_by_key(|unit| {
                let [dx, _, dz] = [0, 1, 2].map(|axis| i64::from(unit.position[axis] - me[axis]));
                (!unit.flags.targets_viewer, dx * dx + dz * dz, unit.id)
            });
        if let Some(unit) = next {
            script.send(ZoneCommand::SelectTarget(Some(unit.entity())));
        }
    }

    let (bag_slot, item) = drop;
    eprintln!(
        "equipping {} from bag slot {bag_slot} after {ticks} ticks",
        item_template(item).unwrap().name
    );
    let before = script.zone.snapshot_for_player(1).unwrap();
    script.send(ZoneCommand::EquipItem { bag_slot });
    script.zone.advance_tick().unwrap();
    let after = script.zone.snapshot_for_player(1).unwrap();
    let slot = usize::from(item_template(item).unwrap().slot.unwrap().index());
    let equipment = after.equipment.expect("equipping sends the sheet");
    assert_eq!(equipment.slots()[slot], Some(item));
    assert_eq!(
        after.inventory.as_ref().unwrap().slots()[usize::from(bag_slot)].map(|stack| stack.item()),
        None
    );
    assert!(after.viewer.level >= 2);
    assert!(
        after.viewer.max_health > before.viewer.max_health
            || after.viewer.damage != before.viewer.damage,
        "the equipped drop changes health or damage"
    );
}
