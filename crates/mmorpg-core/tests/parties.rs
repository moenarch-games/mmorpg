//! Party rewards (#404, Atlas 5.3.6 and 5.4.5): a tapper's eligible party members share kill
//! XP, kill-objective credit and loot eligibility, while strangers and members out of range
//! get nothing; stale directory views fail closed.
mod support;

use std::sync::Arc;

use mmorpg_core::{
    CanonicalZoneSnapshot, CreatureId, CreatureLife, EntityRef, ErrorCode, ItemId, LootClaim,
    LootOutcome, LootTable, Npc, NpcId, NpcRole, PartyMember, PartyRoster, PartyState, Quest,
    QuestEntry, QuestId, QuestObjective, QuestRewards, ZoneCommand, ZoneEvent, ZoneId,
    ZoneSimulation,
};
use support::arena::{self, WOLF};

const FUR: ItemId = ItemId::new(1);
const CREATURE: CreatureId = CreatureId::new(1);
const TARGET: EntityRef = EntityRef::Creature(CREATURE);
const HUNT: QuestId = QuestId::new(1);
const LEADER: u32 = 1;
const MATE: u32 = 2;
const STRANGER: u32 = 3;

fn party(revision: u64, leader: u32, mate: u32) -> PartyState {
    PartyState {
        revision,
        roster: Some(PartyRoster {
            party: 42,
            version: 1,
            members: vec![
                PartyMember {
                    name: "Leader".into(),
                    player: Some(leader),
                    online: true,
                },
                PartyMember {
                    name: "Mate".into(),
                    player: Some(mate),
                    online: true,
                },
            ],
        }),
        invite: None,
    }
}

fn zone() -> ZoneSimulation {
    let content = arena::arena(
        vec![arena::wolf(1, [1, 1])],
        vec![arena::spawn(1, WOLF, [-180, 0])],
        vec![Npc {
            role: NpcRole::QuestGiver,
            ..arena::npc(1, [250, -250])
        }],
    );
    let quest = Quest {
        id: HUNT,
        name: "Hunt".into(),
        text: "Hunt.".into(),
        giver: NpcId::new(1),
        ender: NpcId::new(1),
        prerequisite: None,
        objectives: vec![QuestObjective::Kill {
            template: WOLF,
            count: 2,
        }],
        rewards: QuestRewards::default(),
    };
    let table = LootTable::new(
        [3, 3],
        &[LootOutcome::Item {
            weight: 1,
            item: FUR,
            quantity: [1, 1],
        }],
    )
    .unwrap();
    let content = Arc::new(
        content
            .as_ref()
            .clone()
            .with_quests(vec![quest])
            .unwrap()
            .with_loot_tables(vec![(WOLF, table)])
            .unwrap(),
    );
    let mut zone = ZoneSimulation::with_content(ZoneId::new(1), content).unwrap();
    for player in [LEADER, MATE, STRANGER] {
        zone.add_player(player).unwrap();
    }
    restore(&mut zone, |state| {
        state.creatures[0].health = 1;
        for player in &mut state.players {
            player.quests.entries.push(QuestEntry {
                quest: HUNT,
                progress: Default::default(),
            });
        }
    });
    zone
}

fn restore(zone: &mut ZoneSimulation, change: impl FnOnce(&mut CanonicalZoneSnapshot)) {
    let mut state = zone.snapshot().unwrap();
    change(&mut state);
    *zone = ZoneSimulation::from_snapshot(state, Arc::clone(zone.content())).unwrap();
}

fn command(zone: &mut ZoneSimulation, player: u32, command: ZoneCommand) {
    let sequence = zone
        .snapshot()
        .unwrap()
        .players
        .iter()
        .find(|record| record.player_id == player)
        .unwrap()
        .last_sequence
        + 1;
    zone.apply_command(player, sequence, command).unwrap();
}

fn kill(zone: &mut ZoneSimulation) -> LootClaim {
    command(zone, LEADER, ZoneCommand::SelectTarget(Some(TARGET)));
    for _ in 0..180 {
        if let Some(swing) = arena::swing_at(zone, LEADER, TARGET) {
            command(zone, LEADER, swing);
        }
        zone.advance_tick().unwrap();
        if let CreatureLife::Corpse { died_at } = zone.snapshot().unwrap().creatures[0].life {
            return LootClaim {
                creature: CREATURE,
                died_at,
            };
        }
    }
    panic!("the wolf did not die");
}

fn player_view(zone: &ZoneSimulation, player: u32) -> mmorpg_core::CanonicalPlayerSnapshot {
    zone.snapshot()
        .unwrap()
        .players
        .into_iter()
        .find(|record| record.player_id == player)
        .unwrap()
}

fn progress(zone: &ZoneSimulation, player: u32) -> u8 {
    player_view(zone, player).quests.entries[0].progress[0]
}

fn experience(zone: &ZoneSimulation, player: u32) -> u32 {
    player_view(zone, player).combat.experience
}

#[test]
fn a_shared_kill_credits_party_members_once_and_leaves_strangers_out() {
    let mut zone = zone();
    assert!(
        zone.apply_party_state(LEADER, party(5, LEADER, MATE))
            .unwrap()
    );
    assert!(
        zone.apply_party_state(MATE, party(5, LEADER, MATE))
            .unwrap()
    );
    kill(&mut zone);

    for member in [LEADER, MATE] {
        assert_eq!(
            progress(&zone, member),
            1,
            "player {member} earns the objective"
        );
        assert!(
            experience(&zone, member) > 0,
            "player {member} earns the XP"
        );
    }
    assert_eq!(progress(&zone, STRANGER), 0);
    assert_eq!(experience(&zone, STRANGER), 0);
    // The party is told; the kill is not credited twice by the views that named it.
    let events = zone.snapshot_for_player(MATE).unwrap().events;
    assert!(
        events
            .iter()
            .any(|event| matches!(event, ZoneEvent::Died { .. })),
        "{events:?}"
    );
    assert_eq!(progress(&zone, MATE), 1);
}

#[test]
fn a_member_out_of_range_or_with_a_view_not_yet_updated_shares_nothing() {
    let mut zone = zone();
    assert!(
        zone.apply_party_state(LEADER, party(5, LEADER, MATE))
            .unwrap()
    );
    assert!(
        zone.apply_party_state(MATE, party(5, LEADER, MATE))
            .unwrap()
    );
    restore(&mut zone, |state| {
        let mate = state
            .players
            .iter_mut()
            .find(|player| player.player_id == MATE)
            .unwrap();
        mate.position = mmorpg_core::physics::from_units([9_000, 90, 0]);
    });
    kill(&mut zone);
    assert_eq!(progress(&zone, MATE), 0);
    assert_eq!(experience(&zone, MATE), 0);

    // A leader whose view is not yet updated does not share with someone who has left.
    let mut zone = self::zone();
    assert!(
        zone.apply_party_state(LEADER, party(5, LEADER, MATE))
            .unwrap()
    );
    kill(&mut zone);
    assert_eq!(progress(&zone, MATE), 0);
}

#[test]
fn party_members_can_claim_the_corpse_and_strangers_cannot() {
    let mut zone = zone();
    assert!(
        zone.apply_party_state(LEADER, party(5, LEADER, MATE))
            .unwrap()
    );
    assert!(
        zone.apply_party_state(MATE, party(5, LEADER, MATE))
            .unwrap()
    );
    let claim = kill(&mut zone);

    assert!(
        zone.snapshot_for_player(MATE)
            .unwrap()
            .entities
            .iter()
            .any(|entity| entity.id == CREATURE.get() && entity.flags.lootable)
    );
    // The canonical corpse keeps its eligibility through recovery.
    restore(&mut zone, |state| {
        assert_eq!(state.creatures[0].loot_party, vec![MATE]);
    });
    restore(&mut zone, |state| {
        for player in &mut state.players {
            player.position = state.creatures[0].position;
        }
    });

    command(&mut zone, STRANGER, ZoneCommand::Loot(claim));
    zone.advance_tick().unwrap();
    let events = zone.snapshot_for_player(STRANGER).unwrap().events;
    assert!(
        events.iter().any(|event| matches!(
            event,
            ZoneEvent::Error {
                code: ErrorCode::NotLootOwner,
                ..
            }
        )),
        "{events:?}"
    );

    command(&mut zone, MATE, ZoneCommand::Loot(claim));
    zone.advance_tick().unwrap();
    let mate = player_view(&zone, MATE);
    assert!(mate.copper > 0, "the party member claimed the corpse");

    // Claimed once: the corpse no longer has anything for the leader.
    command(&mut zone, LEADER, ZoneCommand::Loot(claim));
    zone.advance_tick().unwrap();
    let leader = player_view(&zone, LEADER);
    assert_eq!(leader.copper, 0);
}

#[test]
fn stale_party_views_fail_closed_and_the_canonical_view_round_trips() {
    let mut zone = zone();
    assert!(
        zone.apply_party_state(LEADER, party(5, LEADER, MATE))
            .unwrap()
    );
    // Equal or older revisions change nothing.
    assert!(
        !zone
            .apply_party_state(LEADER, party(5, LEADER, MATE))
            .unwrap()
    );
    assert!(
        !zone
            .apply_party_state(LEADER, party(4, LEADER, MATE))
            .unwrap()
    );
    assert!(zone.apply_party_state(99, party(9, LEADER, MATE)).is_err());
    assert_eq!(zone.party_revision(LEADER), 5);

    let state = zone.snapshot().unwrap();
    let restored =
        ZoneSimulation::from_snapshot(state.clone(), Arc::clone(zone.content())).unwrap();
    assert_eq!(restored.party_state(LEADER), zone.party_state(LEADER));
    let mut broken = state;
    broken.parties.push((77, party(1, LEADER, MATE)));
    assert!(ZoneSimulation::from_snapshot(broken, Arc::clone(zone.content())).is_err());

    // The projection carries the view for the viewer's frame.
    let view = zone.snapshot_for_player(LEADER).unwrap();
    assert_eq!(view.party.as_ref().map(|state| state.revision), Some(5));
    assert!(zone.snapshot_for_player(STRANGER).unwrap().party.is_none());
}

#[test]
fn removing_a_player_drops_its_party_view_and_passes_a_corpse_to_the_party() {
    let mut zone = zone();
    assert!(
        zone.apply_party_state(LEADER, party(5, LEADER, MATE))
            .unwrap()
    );
    assert!(
        zone.apply_party_state(MATE, party(5, LEADER, MATE))
            .unwrap()
    );
    kill(&mut zone);
    assert!(zone.remove_player(LEADER));
    assert_eq!(zone.party_revision(LEADER), 0);
    let state = zone.snapshot().unwrap();
    assert_eq!(state.creatures[0].tapped_by, Some(MATE));
    assert!(
        state.creatures[0].loot.is_some(),
        "the loot stays with the party"
    );
}
