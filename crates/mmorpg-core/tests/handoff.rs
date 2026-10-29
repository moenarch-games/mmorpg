//! Seam corridors and the fenced character transfer between two logical
//! zones (#145): one live character across walking and teleport handoffs,
//! idempotent staging and activation, abort back to the source, corridor
//! refusals and creatures turning back before the seam.
mod support;

use std::sync::Arc;

use mmorpg_core::{
    CharacterId, CharacterLocation, CharacterTransfer, ClassChoice, CreatureAi, CreatureId,
    EntityRef, ErrorCode, HandoffCorridor, LootClaim, NpcId, NpcRole, PlayerClass, Sex, TransferId,
    XzBounds, ZoneCommand, ZoneContent, ZoneEvent, ZoneId, ZoneSimulation, starter_vendor_stock,
};
use support::{arena, classes};

const WEST: ZoneId = ZoneId::new(1);
const EAST: ZoneId = ZoneId::new(2);
/// East coordinates are west coordinates shifted 12 km toward −X.
const SHIFT: i32 = 12_000;
const VENDOR: NpcId = NpcId::new(1);

fn eastward() -> u16 {
    mmorpg_core::trig::yaw_from_vector(1, 0).unwrap()
}

/// The west zone's seam: corridor x ∈ [5000, 6000), crossing x ∈ [5800, 6000).
fn west_corridor() -> HandoffCorridor {
    HandoffCorridor {
        neighbor: EAST,
        corridor: XzBounds {
            min: [5_000, -1_000],
            max: [6_000, 1_000],
        },
        crossing: XzBounds {
            min: [5_800, -1_000],
            max: [6_000, 1_000],
        },
        offset: [-SHIFT, 0, 0],
    }
}

/// The east zone's side of the same seam: corridor x ∈ [-6500, -5500), crossing back west at
/// x ∈ [-6500, -6300), so an arrival at x ≥ -6200 is not handed straight back.
fn east_corridor() -> HandoffCorridor {
    HandoffCorridor {
        neighbor: WEST,
        corridor: XzBounds {
            min: [-6_500, -1_000],
            max: [-5_500, 1_000],
        },
        crossing: XzBounds {
            min: [-6_500, -1_000],
            max: [-6_300, 1_000],
        },
        offset: [SHIFT, 0, 0],
    }
}

fn content(corridor: HandoffCorridor, wolf_at: Option<[i32; 2]>) -> Arc<ZoneContent> {
    let (templates, spawns) = match wolf_at {
        Some(at) => (
            vec![arena::wolf(500, [1, 2])],
            vec![arena::spawn(1, arena::WOLF, at)],
        ),
        None => (Vec::new(), Vec::new()),
    };
    let mut vendor = arena::npc(VENDOR.get(), [-300, -600]);
    vendor.role = NpcRole::Vendor;
    let base = ZoneContent::clone(&arena::arena(templates, spawns, vec![vendor]))
        .with_creature_abilities(Vec::new())
        .unwrap()
        .with_vendors(vec![(VENDOR, starter_vendor_stock())])
        .unwrap();
    Arc::new(base.with_handoff_corridors(vec![corridor]).unwrap())
}

fn warden() -> ClassChoice {
    ClassChoice {
        class: PlayerClass::Warden,
        sex: Sex::Female,
    }
}

/// `zone` with `player` moved to the body centre `(x, z)`.
fn placed(zone: &ZoneSimulation, player: u32, x: f32, z: f32) -> ZoneSimulation {
    let mut state = zone.snapshot().unwrap();
    let record = state
        .players
        .iter_mut()
        .find(|record| record.player_id == player)
        .unwrap();
    record.position = [x, 90.0, z];
    ZoneSimulation::from_snapshot(state, Arc::clone(zone.content())).unwrap()
}

fn west_with_character_at(x: f32) -> ZoneSimulation {
    let mut zone = ZoneSimulation::with_content(WEST, content(west_corridor(), None)).unwrap();
    zone.create_character(1, warden()).unwrap();
    placed(&zone, 1, x, 0.0)
}

fn east() -> ZoneSimulation {
    ZoneSimulation::with_content(EAST, content(east_corridor(), None)).unwrap()
}

fn character() -> CharacterId {
    CharacterId::new("seam-walker").unwrap()
}

fn transfer_id(value: u64) -> TransferId {
    TransferId::new(value).unwrap()
}

fn walk_east() -> ZoneCommand {
    ZoneCommand::Move {
        forward: 1,
        strafe: 0,
        facing: eastward(),
    }
}

fn x_of(zone: &ZoneSimulation, player: u32) -> f32 {
    zone.player_motion(player).unwrap().0[0]
}

/// Walks player 1 east until it stands in the west crossing strip.
fn walk_into_crossing(west: &mut ZoneSimulation) {
    west.apply_command(1, 1, walk_east()).unwrap();
    for _ in 0..600 {
        if !west.seam_crossings().is_empty() {
            return;
        }
        west.advance_tick().unwrap();
    }
    panic!("the walker never reached the crossing");
}

#[test]
fn walking_through_a_seam_hands_one_live_character_to_the_neighbour() {
    let mut west = west_with_character_at(5_300.0);
    let mut east = east();
    walk_into_crossing(&mut west);
    let crossing = west.seam_crossings()[0];
    assert_eq!(crossing.player, 1);
    assert_eq!(crossing.destination.zone_id, EAST);
    let before = west.capture_character(1, character()).unwrap();
    let west_x = x_of(&west, 1);

    // Cutover: the source freezes and exports; nobody is live until activation.
    let transfer = west
        .freeze_for_transfer(1, character(), transfer_id(7), crossing.destination)
        .unwrap();
    assert_eq!(west.player_count(), 0);
    assert!(west.apply_command(1, 2, walk_east()).is_err());
    assert_eq!(transfer.cutover_sequence, 1);
    assert_eq!(transfer.motion.forward, 1);
    assert_eq!(transfer.record.location.zone_id, EAST);
    assert_eq!(
        CharacterRecordFacts::of(&transfer.record),
        CharacterRecordFacts::of(&before)
    );

    east.stage_transfer(transfer.clone()).unwrap();
    east.stage_transfer(transfer.clone()).unwrap();
    assert_eq!(east.player_count(), 0);
    east.activate_transfer(transfer_id(7), 40).unwrap();
    let arrived = east.capture_character(40, character()).unwrap();
    assert_eq!(
        CharacterRecordFacts::of(&arrived),
        CharacterRecordFacts::of(&before)
    );
    east.activate_transfer(transfer_id(7), 40).unwrap();
    east.stage_transfer(transfer.clone()).unwrap();
    assert_eq!(east.player_count(), 1);
    assert!(west.retire_transfer(transfer_id(7)));
    assert!(!west.retire_transfer(transfer_id(7)));
    assert!(west.abort_transfer(transfer_id(7)).is_err());
    assert_eq!(west.player_count() + east.player_count(), 1);

    // The same place, still walking: commands up to the cutover stay stale.
    let east_x = x_of(&east, 40);
    assert!(
        (east_x - (west_x - SHIFT as f32)).abs() <= 1.0,
        "{east_x} vs {west_x}"
    );
    assert!(east.apply_command(40, 1, walk_east()).is_err());
    east.advance_tick().unwrap();
    assert!(
        x_of(&east, 40) > east_x,
        "movement continues across the seam"
    );
    east.apply_command(40, 2, walk_east()).unwrap();
    // Arriving inside the east corridor but short of its crossing, the walker is not sent back.
    assert!(east.seam_crossings().is_empty());
}

/// The durable facts a transfer must carry unchanged (everything but location).
#[derive(Debug, PartialEq)]
struct CharacterRecordFacts(mmorpg_core::CharacterRecord);

impl CharacterRecordFacts {
    fn of(record: &mmorpg_core::CharacterRecord) -> Self {
        let mut record = record.clone();
        record.location = CharacterLocation {
            zone_id: ZoneId::new(0),
            position: [0; 3],
            facing: 0,
        };
        Self(record)
    }
}

#[test]
fn a_refused_transfer_aborts_back_to_an_unchanged_source() {
    let mut west = west_with_character_at(5_900.0);
    west.apply_command(1, 1, walk_east()).unwrap();
    west.advance_tick().unwrap();
    let before = west.snapshot().unwrap();
    let destination = west.seam_crossings()[0].destination;
    let transfer = west
        .freeze_for_transfer(1, character(), transfer_id(9), destination)
        .unwrap();

    // A destination that is not the addressed zone refuses to stage.
    let mut wrong =
        ZoneSimulation::with_content(ZoneId::new(3), content(east_corridor(), None)).unwrap();
    assert!(wrong.stage_transfer(transfer.clone()).is_err());
    assert_eq!(wrong.player_count(), 0);

    assert_eq!(west.outbound_transfer(transfer_id(9)), Some(&transfer));
    assert_eq!(west.abort_transfer(transfer_id(9)).unwrap(), 1);
    assert_eq!(west.snapshot().unwrap(), before);
    assert!(west.outbound_transfer(transfer_id(9)).is_none());
    assert!(!west.retire_transfer(transfer_id(9)));
    // The source stays authoritative and fenced at the cutover sequence.
    assert!(west.apply_command(1, 1, walk_east()).is_err());
    west.apply_command(1, 2, walk_east()).unwrap();
}

#[test]
fn staging_and_activation_are_fenced_by_the_transfer_id() {
    let mut west = west_with_character_at(5_900.0);
    let destination = west.seam_crossings()[0].destination;
    let transfer = west
        .freeze_for_transfer(1, character(), transfer_id(11), destination)
        .unwrap();
    assert!(
        west.freeze_for_transfer(1, character(), transfer_id(11), destination)
            .is_err()
    );
    let mut east = east();
    assert!(east.activate_transfer(transfer_id(11), 5).is_err());
    east.stage_transfer(transfer.clone()).unwrap();

    let mut forged = transfer.clone();
    forged.record.copper += 1_000;
    assert!(east.stage_transfer(forged.clone()).is_err());
    forged.transfer_id = transfer_id(12);
    assert!(
        east.stage_transfer(forged).is_err(),
        "a character is staged once"
    );

    east.activate_transfer(transfer_id(11), 5).unwrap();
    assert!(east.activate_transfer(transfer_id(11), 6).is_err());
    assert_eq!(east.player_count(), 1);

    // A staged transfer the control plane aborted is dropped without a live character.
    let mut other = ZoneSimulation::with_content(EAST, content(east_corridor(), None)).unwrap();
    other.stage_transfer(transfer).unwrap();
    assert!(other.discard_staged_transfer(transfer_id(11)));
    assert!(other.activate_transfer(transfer_id(11), 5).is_err());
    assert_eq!(other.player_count(), 0);
}

#[test]
fn a_teleport_uses_the_same_transfer_without_a_corridor() {
    let mut west = west_with_character_at(1_000.0);
    assert!(west.seam_crossings().is_empty());
    let waystone = CharacterLocation {
        zone_id: EAST,
        position: [2_000, 90, 2_000],
        facing: 0,
    };
    assert!(
        west.freeze_for_transfer(
            1,
            character(),
            transfer_id(3),
            CharacterLocation {
                zone_id: WEST,
                ..waystone
            }
        )
        .is_err(),
        "a transfer leaves its zone"
    );
    let transfer: CharacterTransfer = west
        .freeze_for_transfer(1, character(), transfer_id(3), waystone)
        .unwrap();
    let mut east = east();
    east.stage_transfer(transfer).unwrap();
    east.activate_transfer(transfer_id(3), 2).unwrap();
    assert!(west.retire_transfer(transfer_id(3)));
    let (position, _) = east.player_motion(2).unwrap();
    assert_eq!(position, [2_000.0, 90.0, 2_000.0]);
}

fn refusals(zone: &mut ZoneSimulation, sequence: &mut u32, command: ZoneCommand) -> Vec<ErrorCode> {
    *sequence += 1;
    zone.apply_command(1, *sequence, command).unwrap();
    zone.advance_tick().unwrap();
    zone.snapshot_for_player(1)
        .unwrap()
        .events
        .into_iter()
        .filter_map(|event| match event {
            ZoneEvent::Error { code, .. } => Some(code),
            _ => None,
        })
        .collect()
}

#[test]
fn corridors_refuse_loot_trades_and_quest_changes() {
    let commands = [
        ZoneCommand::Loot(LootClaim {
            creature: CreatureId::new(1),
            died_at: 0,
        }),
        ZoneCommand::BuyItem {
            npc: VENDOR,
            offer: 0,
            quantity: 1,
        },
        ZoneCommand::SellItem {
            npc: VENDOR,
            bag_slot: 0,
            quantity: 1,
        },
        ZoneCommand::AcceptQuest {
            npc: VENDOR,
            quest: 1,
        },
        ZoneCommand::CompleteQuest {
            npc: VENDOR,
            quest: 1,
            choice: 0,
        },
        ZoneCommand::AbandonQuest { quest: 1 },
    ];
    let mut inside = west_with_character_at(5_400.0);
    let mut outside = west_with_character_at(4_000.0);
    let (mut inside_sequence, mut outside_sequence) = (0, 0);
    for command in commands {
        assert_eq!(
            refusals(&mut inside, &mut inside_sequence, command),
            [ErrorCode::HandoffCorridor],
            "{command:?}"
        );
        assert_ne!(
            refusals(&mut outside, &mut outside_sequence, command),
            [ErrorCode::HandoffCorridor],
            "{command:?}"
        );
    }
}

fn wolf_ai(zone: &ZoneSimulation) -> (CreatureAi, f32) {
    let state = zone.snapshot().unwrap();
    let wolf = &state.creatures[0];
    (wolf.ai, wolf.position[0])
}

#[test]
fn creatures_evade_attacks_from_a_corridor_and_never_follow_into_it() {
    // The wolf stands just clear of the corridor and its turn-back margin.
    let content = content(west_corridor(), Some([4_750, 0]));
    let mut zone = ZoneSimulation::with_content(WEST, Arc::clone(&content)).unwrap();
    zone.create_character(1, warden()).unwrap();

    // A player inside the corridor neither draws it nor hurts it.
    let mut zone = placed(&zone, 1, 5_000.0, 0.0);
    let wolf = EntityRef::Creature(CreatureId::new(1));
    zone.advance_tick().unwrap();
    assert!(matches!(wolf_ai(&zone).0, CreatureAi::Idle { .. }));
    let swing = arena::swing_at(&zone, 1, wolf).expect("the wolf is within reach");
    zone.apply_command(1, 1, swing).unwrap();
    let mut evaded = false;
    for _ in 0..90 {
        zone.advance_tick().unwrap();
        evaded |= zone
            .snapshot_for_player(1)
            .unwrap()
            .events
            .iter()
            .any(|event| matches!(event, ZoneEvent::Evade { target, .. } if *target == wolf));
    }
    assert!(evaded, "the corridor attack is evaded");
    assert!(matches!(wolf_ai(&zone).0, CreatureAi::Idle { .. }));

    // Engaged outside, it gives up once its target steps into the corridor.
    let mut zone = placed(&zone, 1, 4_500.0, 0.0);
    zone.advance_tick().unwrap();
    assert_eq!(wolf_ai(&zone).0, CreatureAi::Engaged);
    zone.apply_command(1, 2, walk_east()).unwrap();
    let mut gave_up = false;
    for _ in 0..120 {
        zone.advance_tick().unwrap();
        let (ai, x) = wolf_ai(&zone);
        assert!(x < 5_000.0, "the wolf entered the corridor at x = {x}");
        gave_up |= matches!(ai, CreatureAi::Evading { .. });
    }
    assert!(gave_up);
}

#[test]
fn content_keeps_every_unit_clear_of_corridors() {
    let base = || ZoneContent::clone(&classes::arena(Vec::new(), Vec::new()));
    // A wanderer whose area reaches the turn-back margin.
    let wanderer = ZoneContent::clone(&classes::arena(
        vec![arena::wolf(10, [1, 1])],
        vec![mmorpg_core::CreatureSpawn {
            wander_radius: 300,
            ..arena::spawn(1, arena::WOLF, [4_500, 0])
        }],
    ));
    assert!(
        wanderer
            .with_handoff_corridors(vec![west_corridor()])
            .is_err()
    );
    // A player spawn slot inside the corridor.
    let mut over_spawn = west_corridor();
    over_spawn.corridor.min = [0, -1_000];
    assert!(base().with_handoff_corridors(vec![over_spawn]).is_err());
    // A crossing outside its corridor, and overlapping corridors.
    let mut stray = west_corridor();
    stray.crossing.max = [7_000, 1_000];
    assert!(base().with_handoff_corridors(vec![stray]).is_err());
    assert!(
        base()
            .with_handoff_corridors(vec![west_corridor(), west_corridor()])
            .is_err()
    );
    // Corridors are part of the content identity; content without them keeps its revision.
    let plain = base();
    let seamed = base()
        .with_handoff_corridors(vec![west_corridor()])
        .unwrap();
    assert_ne!(plain.revision(), seamed.revision());
    assert_eq!(
        plain.revision(),
        base()
            .with_handoff_corridors(Vec::new())
            .unwrap()
            .revision()
    );
}
