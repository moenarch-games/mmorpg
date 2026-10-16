//! Continue an active quest, its pending intents and a quest-item corpse
//! through real canonical bytes on the hosted Greyhaven content.
use std::sync::Arc;

use mmorpg_core::greyhaven_vale::quests::{MARSHAL, TANNER, TROUBLE_IN_THE_WOODS, WOLF_PELT};
use mmorpg_core::{
    CreatureId, CreatureLife, EntityRef, ZoneCommand, ZoneId, ZoneSimulation, greyhaven_vale,
};
use mmorpg_protocol::{
    decode_canonical_snapshot, decode_snapshot, encode_canonical_snapshot, pack_snapshot,
};

/// Timber Wolf 108 guards the den clearing of the Wolfrun Woods.
const DEN_WOLF: CreatureId = CreatureId::new(108);

fn recovered(zone: &ZoneSimulation) -> ZoneSimulation {
    let bytes = encode_canonical_snapshot(&zone.snapshot().unwrap()).unwrap();
    ZoneSimulation::from_snapshot(
        decode_canonical_snapshot(&bytes).unwrap(),
        Arc::clone(zone.content()),
    )
    .unwrap()
}

/// Moves player 1 to `feet` through a restored canonical snapshot (setup only).
fn teleported(zone: &ZoneSimulation, feet: [i32; 2]) -> ZoneSimulation {
    let mut state = zone.snapshot().unwrap();
    state.players[0].position = [feet[0], 90, feet[1]];
    state.players[0].velocity = [0; 3];
    ZoneSimulation::from_snapshot(state, Arc::clone(zone.content())).unwrap()
}

fn continue_equally(left: &mut ZoneSimulation, right: &mut ZoneSimulation, ticks: usize) {
    for _ in 0..ticks {
        left.advance_tick().unwrap();
        right.advance_tick().unwrap();
        assert_eq!(
            encode_canonical_snapshot(&left.snapshot().unwrap()).unwrap(),
            encode_canonical_snapshot(&right.snapshot().unwrap()).unwrap()
        );
        let packed = pack_snapshot(&left.snapshot_for_player(1).unwrap()).unwrap();
        assert_eq!(
            packed,
            pack_snapshot(&right.snapshot_for_player(1).unwrap()).unwrap()
        );
        decode_snapshot(&packed.payload).unwrap();
    }
}

#[test]
fn an_active_quest_and_its_pending_intents_continue_through_canonical_bytes() {
    let mut zone = ZoneSimulation::with_content(ZoneId::new(1), greyhaven_vale::content()).unwrap();
    zone.add_player(1).unwrap();
    // Beside the Marshal; the Tanner's quest is still locked and out of reach.
    let mut zone = teleported(&zone, [-2_200, 1_150]);
    for (sequence, command) in [
        ZoneCommand::AcceptQuest {
            npc: MARSHAL,
            quest: TROUBLE_IN_THE_WOODS.get(),
        },
        ZoneCommand::AcceptQuest {
            npc: TANNER,
            quest: 2,
        },
    ]
    .into_iter()
    .enumerate()
    {
        zone.apply_command(1, u32::try_from(sequence).unwrap() + 1, command)
            .unwrap();
    }
    let mut restored = recovered(&zone);
    continue_equally(&mut zone, &mut restored, 12);
    let player = zone.snapshot().unwrap().players.remove(0);
    assert_eq!(player.quests.entries.len(), 1, "only the Marshal's quest");

    // With Trouble in the Woods turned in (setup), Pelts for the Tanner is
    // accepted beside the Tanner; a den wolf kill then leaves its quest-only
    // pelt on the corpse, and the corpse continues across recovery.
    let mut state = zone.snapshot().unwrap();
    state.players[0].quests.entries.clear();
    state.players[0].quests.completed = 0b1;
    let zone = ZoneSimulation::from_snapshot(state, Arc::clone(zone.content())).unwrap();
    let mut zone = teleported(&zone, [-2_400, 2_850]);
    zone.apply_command(
        1,
        3,
        ZoneCommand::AcceptQuest {
            npc: TANNER,
            quest: 2,
        },
    )
    .unwrap();
    zone.advance_tick().unwrap();
    assert_eq!(zone.snapshot().unwrap().players[0].quests.entries.len(), 1);
    let zone = teleported(&zone, [-8_450, 250]);
    // One swing kills the den wolf (setup), so the player never dies.
    let mut state = zone.snapshot().unwrap();
    for record in &mut state.creatures {
        if record.creature_id == DEN_WOLF {
            record.health = 1;
        }
    }
    let mut zone = ZoneSimulation::from_snapshot(state, Arc::clone(zone.content())).unwrap();
    zone.apply_command(
        1,
        4,
        ZoneCommand::SelectTarget(Some(EntityRef::Creature(DEN_WOLF))),
    )
    .unwrap();
    zone.apply_command(1, 5, ZoneCommand::StartAttack).unwrap();
    let mut restored = recovered(&zone);
    for _ in 0..1_200 {
        continue_equally(&mut zone, &mut restored, 1);
        let state = zone.snapshot().unwrap();
        let wolf = state
            .creatures
            .iter()
            .find(|record| record.creature_id == DEN_WOLF)
            .unwrap();
        if matches!(wolf.life, CreatureLife::Corpse { .. }) {
            let rewards = wolf.loot.unwrap();
            assert_eq!(
                rewards.quest_item.map(|stack| stack.item()),
                Some(WOLF_PELT)
            );
            let mut again = recovered(&zone);
            continue_equally(&mut zone, &mut again, 30);
            return;
        }
    }
    panic!("the den wolf survived");
}
