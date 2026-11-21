//! The same Greyhaven group fight with a sword and with a spear (#914): a level 4 Warden pulls
//! the Eastwater Fen wolf pack (three Timber Wolves) and holds its basic strike at the nearest
//! wolf, once with its own sword and once after equipping the Militia Spear. The pack closes to
//! its bite range (about 2.2 m, inside both weapons' reach), so in this crush the families differ
//! by timing and arc: the sword lands first and strikes more often, its 50° sweep catches two
//! crowding wolves more often than the spear's 30° thrust, and the spear's heavier, slower
//! strikes finish the pack sooner. Which far or off-axis units each family can reach is
//! `weapon_families.rs`. The fight replays identically.
mod support;

use std::sync::Arc;

use mmorpg_core::greyhaven_vale::{self, fen_pack};
use mmorpg_core::{
    CreatureId, EntityRef, Inventory, ItemId, ItemStack, WeaponFamily, ZoneCommand, ZoneEvent,
    ZoneId, ZoneSimulation,
};
use support::{arena, classes};

const PLAYER: u32 = 1;
const WARDEN: u8 = 0;
const MILITIA_SPEAR: ItemId = ItemId::new(23);
/// An empty bag slot for the spear.
const SPEAR_SLOT: u8 = 2;
/// North of the lead wolf, 3 m from it, inside its pull.
const START: [i32; 2] = [16_000, 10_300];
const PACK: [CreatureId; 3] = [fen_pack::LEAD, fen_pack::FOLLOWER, fen_pack::FLANKER];
/// How long the scripted fight runs.
const FIGHT_TICKS: u64 = 600;

/// What one fight produced, all read from the player's own projection.
#[derive(Clone, Debug, PartialEq)]
struct Fight {
    family: Option<WeaponFamily>,
    /// Basic strikes begun.
    strikes: u32,
    /// Strike contacts that struck at least one wolf, and those that struck two or more.
    connected: u32,
    multi: u32,
    /// The tick of the first hit.
    first_hit: Option<u64>,
    /// Damage dealt and taken; wolves killed.
    dealt: u32,
    taken: u32,
    kills: u32,
    end: mmorpg_core::CanonicalZoneSnapshot,
}

/// A level 4 Warden at [`START`] with the Militia Spear in its bag.
fn warden() -> ZoneSimulation {
    let content = greyhaven_vale::content();
    let mut zone = ZoneSimulation::with_content(ZoneId::new(9), Arc::clone(&content)).unwrap();
    zone.add_player(PLAYER).unwrap();
    zone.apply_command(
        PLAYER,
        1,
        ZoneCommand::ChooseClass {
            class: WARDEN,
            sex: 0,
        },
    )
    .unwrap();
    zone.advance_tick().unwrap();
    let zone = classes::at_level(zone, PLAYER, 4);
    let mut state = zone.snapshot().unwrap();
    let ground = greyhaven_vale::terrain().height_at(START[0], START[1]);
    let player = &mut state.players[0];
    player.position = [START[0] as f32, (ground + 95) as f32, START[1] as f32];
    player.velocity = [0.0; 3];
    let mut slots = *player.inventory.slots();
    assert_eq!(slots[usize::from(SPEAR_SLOT)], None);
    slots[usize::from(SPEAR_SLOT)] = Some(ItemStack::new(MILITIA_SPEAR, 1).unwrap());
    player.inventory = Inventory::from_slots(slots);
    ZoneSimulation::from_snapshot(state, content).unwrap()
}

/// The nearest living pack wolf in view: its facing from the player and squared distance.
fn nearest(zone: &ZoneSimulation, dead: &[EntityRef]) -> Option<(u16, i64)> {
    PACK.iter()
        .map(|&id| EntityRef::Creature(id))
        .filter(|wolf| !dead.contains(wolf))
        .filter_map(|wolf| arena::aim_at(zone, PLAYER, wolf))
        .min_by_key(|&(_, distance)| distance)
}

/// The scripted fight: optionally equip the spear, then hold the basic strike at the nearest
/// wolf, turning toward it between presses, as a player holding Q and steering would.
fn fight(spear: bool) -> Fight {
    let mut zone = warden();
    let mut sequence = 1;
    let mut send = |zone: &mut ZoneSimulation, command| {
        sequence += 1;
        zone.apply_command(PLAYER, sequence, command).unwrap();
    };
    if spear {
        send(
            &mut zone,
            ZoneCommand::EquipItem {
                bag_slot: SPEAR_SLOT,
            },
        );
        zone.advance_tick().unwrap();
        assert!(
            !zone
                .snapshot_for_player(PLAYER)
                .unwrap()
                .events
                .iter()
                .any(|event| matches!(event, ZoneEvent::Error { .. })),
            "the Warden equips the spear"
        );
    }
    let mut result = Fight {
        family: None,
        strikes: 0,
        connected: 0,
        multi: 0,
        first_hit: None,
        dealt: 0,
        taken: 0,
        kills: 0,
        end: zone.snapshot().unwrap(),
    };
    let mut dead = Vec::new();
    // A press waits for its contact before the next one.
    let mut pressed = false;
    for _ in 0..FIGHT_TICKS {
        if let Some((facing, _)) = nearest(&zone, &dead) {
            if pressed {
                send(&mut zone, arena::stand(facing));
            } else {
                send(&mut zone, ZoneCommand::Swing { facing });
                pressed = true;
            }
        }
        zone.advance_tick().unwrap();
        let view = zone.snapshot_for_player(PLAYER).unwrap();
        let mut struck = 0;
        for event in &view.events {
            match *event {
                ZoneEvent::StrikeBegan {
                    source: EntityRef::Player(PLAYER),
                    family,
                    ..
                } => {
                    result.family = Some(family);
                    result.strikes += 1;
                }
                ZoneEvent::AttackPerformed {
                    source: EntityRef::Player(PLAYER),
                    ..
                } => pressed = false,
                ZoneEvent::DamageDealt {
                    source: EntityRef::Player(PLAYER),
                    amount,
                    ..
                } => {
                    pressed = false;
                    struck += 1;
                    result.dealt += u32::from(amount);
                    result.first_hit.get_or_insert(view.tick);
                }
                ZoneEvent::DamageTaken { amount, .. } => result.taken += u32::from(amount),
                ZoneEvent::Died {
                    entity,
                    killer: Some(EntityRef::Player(PLAYER)),
                } => {
                    result.kills += 1;
                    dead.push(entity);
                }
                _ => {}
            }
        }
        if struck > 0 {
            result.connected += 1;
        }
        if struck > 1 {
            result.multi += 1;
        }
    }
    result.end = zone.snapshot().unwrap();
    result
}

#[test]
fn sword_and_spear_fight_the_fen_pack_differently() {
    let sword = fight(false);
    let spear = fight(true);
    assert_eq!(sword.family, Some(WeaponFamily::Sword));
    assert_eq!(spear.family, Some(WeaponFamily::Spear));
    // The sword's shorter wind-up lands first and its quicker recovery strikes more often.
    assert!(
        sword.first_hit < spear.first_hit,
        "{} | {}",
        sword.summary(),
        spear.summary()
    );
    assert!(
        sword.strikes > spear.strikes,
        "{} | {}",
        sword.summary(),
        spear.summary()
    );
    // Its wider sweep catches two crowding wolves in one blow more often.
    assert!(
        sword.multi > spear.multi,
        "{} | {}",
        sword.summary(),
        spear.summary()
    );
    // The spear's heavier thrusts kill more of the pack in the same time.
    assert!(
        spear.kills > sword.kills,
        "{} | {}",
        sword.summary(),
        spear.summary()
    );
    assert!(
        spear.dealt > sword.dealt,
        "{} | {}",
        sword.summary(),
        spear.summary()
    );
    // Replaying the same inputs gives the same fight.
    assert_eq!(fight(false), sword);
    assert_eq!(fight(true), spear);
}

impl Fight {
    fn summary(&self) -> String {
        format!(
            "strikes {} connected {} multi {} first {:?} dealt {} taken {} kills {}",
            self.strikes,
            self.connected,
            self.multi,
            self.first_hit,
            self.dealt,
            self.taken,
            self.kills
        )
    }
}
