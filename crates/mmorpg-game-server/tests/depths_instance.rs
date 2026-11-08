//! Private Redbrand Depths runs on a host (#404 part 2), through the adapter and the run
//! directory: the same host inputs a live zone applies. Cross-host transfer is
//! `host_transfer.rs`'s subject; here the arrival goes straight into the slot's adapter.

use std::collections::BTreeMap;

use game_server::GameSimulation;
use mmorpg_control_plane::instance::{EntryCandidate, InstanceDirectory};
use mmorpg_core::greyhaven_vale::{self, cave, quests};
use mmorpg_core::{
    CharacterId, CharacterTransfer, ClassChoice, DepthsRunStart, ErrorCode, PartyMember,
    PartyRoster, PartyState, PlayerClass, Sex, TransferId, ZoneEvent, ZoneId, ZoneSimulation,
    depths_arrival, redbrand_depths,
};
use mmorpg_game_server::ZoneGameServerAdapter;
use mmorpg_game_server::instance::{
    InstanceChannel, ThresholdEntry, bind_run_input, enter_depths_input, refuse_input,
};

const GREYHAVEN: ZoneId = ZoneId::new(1);
const SLOT: ZoneId = ZoneId::new(20_000);
const CLASS: ClassChoice = ClassChoice {
    class: PlayerClass::Warden,
    sex: Sex::Female,
};
/// Inside the threshold strip, three metres from the door's Brute: nothing has struck yet.
const STRIP: [i32; 2] = [7_890, -13_700];

fn character(name: &str) -> CharacterId {
    CharacterId::new(name).unwrap()
}

/// A Greyhaven adapter with claimed characters standing in the threshold strip.
fn public_zone(players: &[(u32, &str, bool)]) -> (ZoneGameServerAdapter, InstanceChannel) {
    let mut adapter =
        ZoneGameServerAdapter::with_content(GREYHAVEN, greyhaven_vale::content()).unwrap();
    let channel = InstanceChannel::default();
    adapter.attach_instances(channel.clone());
    adapter.require_durable_character_claims();
    let mut creator = ZoneSimulation::with_content(GREYHAVEN, greyhaven_vale::content()).unwrap();
    for (index, &(player, name, qualified)) in players.iter().enumerate() {
        adapter.add_player(player).unwrap();
        creator.create_character(1000 + player, CLASS).unwrap();
        let mut record = creator
            .capture_character(1000 + player, character(name))
            .unwrap();
        record.location.position = [
            STRIP[0],
            cave::GREYFANG.floor_y + mmorpg_core::PLAYER_HALF_EXTENTS_UNITS[1],
            STRIP[1] + i32::try_from(index).unwrap() * 80,
        ];
        if qualified {
            record.level = 6;
            record.experience = 0;
            record.health = mmorpg_core::unit::player_max_health(6);
            record.quests.completed = (1..=quests::GARRICK_REDBRAND_QUEST.get())
                .filter_map(|id| mmorpg_core::QuestId::new(id).bit())
                .fold(0, |mask, bit| mask | bit);
        }
        let input = ZoneGameServerAdapter::character_admission_input(&record).unwrap();
        adapter.apply_host_input(player, &input).unwrap();
    }
    (adapter, channel)
}

fn tick(adapter: &mut ZoneGameServerAdapter) {
    adapter.advance_tick().unwrap();
}

fn candidates(entry: &ThresholdEntry) -> Vec<EntryCandidate> {
    entry
        .candidates
        .iter()
        .map(|(character, qualified)| EntryCandidate {
            character: character.clone(),
            qualified: *qualified,
        })
        .collect()
}

fn directory() -> InstanceDirectory {
    let mut directory = InstanceDirectory::new();
    directory.bind_slots([SLOT, ZoneId::new(20_001)]);
    directory
}

fn party_view(members: &[(u32, &str)], revision: u64) -> PartyState {
    PartyState {
        revision,
        roster: Some(PartyRoster {
            party: 7,
            version: 1,
            members: members
                .iter()
                .map(|(player, name)| PartyMember {
                    name: (*name).to_owned(),
                    player: Some(*player),
                    online: true,
                })
                .collect(),
        }),
        invite: None,
    }
}

fn notices(adapter: &ZoneGameServerAdapter, player: u32) -> Vec<ErrorCode> {
    adapter
        .zone()
        .snapshot_for_player(player)
        .unwrap()
        .events
        .iter()
        .filter_map(|event| match event {
            ZoneEvent::Error { code, .. } => Some(*code),
            _ => None,
        })
        .collect()
}

/// Binds `run` to a fresh slot and brings the frozen transfer's character in as `player`.
fn arrive(
    slot: &mut ZoneGameServerAdapter,
    player: u32,
    transfer: &CharacterTransfer,
    start: &DepthsRunStart,
) -> Result<(), game_server::SimulationError> {
    slot.add_player(player)?;
    slot.apply_host_input(player, &bind_run_input(start).unwrap())?;
    slot.apply_host_input(
        player,
        &mmorpg_game_server::arrival_input(transfer).unwrap(),
    )
}

fn transfer_of(
    public: &mut ZoneGameServerAdapter,
    player: u32,
    name: &str,
    slot: ZoneId,
) -> CharacterTransfer {
    public
        .zone_mut()
        .freeze_for_transfer(
            player,
            character(name),
            TransferId::new(u64::from(player) + 1).unwrap(),
            depths_arrival(slot),
        )
        .unwrap()
}

fn standing_creatures(slot: &ZoneGameServerAdapter) -> usize {
    slot.zone()
        .snapshot()
        .unwrap()
        .creatures
        .iter()
        .filter(|creature| creature.life == mmorpg_core::CreatureLife::Alive)
        .count()
}

fn slot_adapter(zone: ZoneId) -> ZoneGameServerAdapter {
    let mut adapter =
        ZoneGameServerAdapter::with_content(zone, redbrand_depths::content()).unwrap();
    adapter.attach_instances(InstanceChannel::default());
    adapter
}

#[test]
fn a_qualified_solo_player_enters_their_own_run_and_an_unqualified_one_is_refused() {
    let (mut public, channel) = public_zone(&[(1, "aria", true), (2, "rook", false)]);
    tick(&mut public);
    let requests = channel.take_requests();
    assert_eq!(requests.len(), 2);
    let mut directory = directory();

    // The unqualified character is refused with a notice and not asked again from the strip.
    let refused = directory
        .enter(&candidates(&requests[1]), 0)
        .expect_err("rook has not ended Garrick");
    assert_eq!(refused, ErrorCode::DepthsLocked);
    public.apply_host_input(2, &refuse_input(refused)).unwrap();
    assert_eq!(
        (notices(&public, 2), channel.take_requests().len()),
        (Vec::new(), 0),
        "the notice waits for the next tick"
    );
    tick(&mut public);
    assert_eq!(notices(&public, 2), [ErrorCode::DepthsLocked]);
    let again = channel.take_requests();
    assert_eq!(
        again.len(),
        1,
        "the refused character is held back: {again:?}"
    );

    // The qualified one gets a run and departs toward its slot.
    let entered = directory.enter(&candidates(&requests[0]), 0).unwrap();
    assert!(entered.created);
    public
        .apply_host_input(1, &enter_depths_input(depths_arrival(entered.run.slot)))
        .unwrap();
    assert_eq!(public.zone().departures().len(), 1);
}

#[test]
fn a_party_of_five_gets_a_larger_different_run_than_a_solo_player() {
    let members: Vec<_> = ["aria", "bren", "cole", "dana", "elis"]
        .iter()
        .enumerate()
        .map(|(index, name)| (u32::try_from(index).unwrap() + 1, *name, index == 0))
        .collect();
    let (mut public, channel) = public_zone(&members);
    let view: Vec<_> = members
        .iter()
        .map(|&(player, name, _)| (player, name))
        .collect();
    for (player, _, _) in &members {
        public
            .zone_mut()
            .apply_party_state(*player, party_view(&view, 1))
            .unwrap();
    }
    tick(&mut public);
    let requests = channel.take_requests();
    let leader = requests.iter().find(|entry| entry.player == 1).unwrap();
    assert_eq!(leader.candidates.len(), 5);

    let mut directory = directory();
    let entered = directory.enter(&candidates(leader), 0).unwrap();
    assert_eq!(entered.run.roster.len(), 5);
    let start = DepthsRunStart {
        run_id: entered.run.id,
        roster: entered.run.roster.clone(),
        cleared: 0,
        checkpoint: false,
    };

    let mut slot = slot_adapter(entered.run.slot);
    let transfer = transfer_of(&mut public, 1, "aria", entered.run.slot);
    arrive(&mut slot, 1, &transfer, &start).unwrap();
    let five = standing_creatures(&slot);

    let (mut solo_public, _) = public_zone(&[(1, "aria", true)]);
    tick(&mut solo_public);
    let mut solo_slot = slot_adapter(ZoneId::new(20_001));
    let solo_start = DepthsRunStart {
        run_id: 99,
        roster: vec![character("aria")],
        cleared: 0,
        checkpoint: false,
    };
    let solo_transfer = transfer_of(&mut solo_public, 1, "aria", ZoneId::new(20_001));
    arrive(&mut solo_slot, 1, &solo_transfer, &solo_start).unwrap();
    let one = standing_creatures(&solo_slot);
    assert!(five > one, "{five} vs {one}");

    // Two unrelated parties are isolated: no enemy or roster leaks between the slots.
    assert_eq!(slot.zone().depths_run().unwrap().run_id, entered.run.id);
    assert_eq!(solo_slot.zone().depths_run().unwrap().run_id, 99);
}

#[test]
fn the_roster_and_difficulty_stay_frozen_and_no_one_joins_late() {
    let (mut public, channel) = public_zone(&[(1, "aria", true)]);
    tick(&mut public);
    let requests = channel.take_requests();
    let mut directory = directory();
    let entered = directory.enter(&candidates(&requests[0]), 0).unwrap();
    let start = DepthsRunStart {
        run_id: entered.run.id,
        roster: entered.run.roster.clone(),
        cleared: 0,
        checkpoint: false,
    };
    let mut slot = slot_adapter(entered.run.slot);
    let transfer = transfer_of(&mut public, 1, "aria", entered.run.slot);
    arrive(&mut slot, 1, &transfer, &start).unwrap();
    let difficulty = slot.zone().depths_run().unwrap().profile();

    // A late character the directory never added cannot arrive, even with a free slot.
    let (mut other_public, _) = public_zone(&[(9, "tess", true)]);
    tick(&mut other_public);
    let late = transfer_of(&mut other_public, 9, "tess", entered.run.slot);
    assert!(arrive(&mut slot, 9, &late, &start).is_err());
    assert_eq!(slot.zone().depths_run().unwrap().profile(), difficulty);

    // A party member the directory never placed on the roster reaches no run: a late
    // candidate who is not bound starts its own new run instead of joining, so the frozen
    // run's roster never changes.
    let attempt = directory
        .enter(
            &[EntryCandidate {
                character: character("dana"),
                qualified: false,
            }],
            5,
        )
        .unwrap_err();
    assert_eq!(attempt, ErrorCode::DepthsLocked);
    assert_eq!(
        directory.run_of(&character("aria")).unwrap().roster.len(),
        1
    );
}

#[test]
fn a_disconnected_member_resumes_the_same_run_and_an_expired_run_retires() {
    let (mut public, channel) = public_zone(&[(1, "aria", true)]);
    tick(&mut public);
    let requests = channel.take_requests();
    let mut directory = directory();
    let entered = directory.enter(&candidates(&requests[0]), 0).unwrap();
    let start = DepthsRunStart {
        run_id: entered.run.id,
        roster: entered.run.roster.clone(),
        cleared: 0,
        checkpoint: false,
    };
    let mut slot = slot_adapter(entered.run.slot);
    let transfer = transfer_of(&mut public, 1, "aria", entered.run.slot);
    arrive(&mut slot, 1, &transfer, &start).unwrap();

    // The character disconnects: the directory keeps the run for the grace period and the
    // same roster member reaches the same run (and slot) from the threshold.
    assert!(GameSimulation::remove_player(&mut slot, 1));
    directory.sync_slot(entered.run.slot, false, 1_000);
    let resumed = directory
        .enter(
            &[EntryCandidate {
                character: character("aria"),
                qualified: false,
            }],
            2_000,
        )
        .unwrap();
    assert_eq!(resumed.run.id, entered.run.id);
    assert!(
        !resumed.created,
        "no second run while one holds the character"
    );

    // Grace passes with nobody back: the run retires safely and the character starts anew.
    directory.sync_slot(entered.run.slot, false, 2_000);
    assert_eq!(directory.sweep(2_000 + 900_000, 900_000), 1);
    assert_eq!(directory.run_count(), 0);
    assert!(directory.run_of(&character("aria")).is_none());
    let fresh = directory
        .enter(&candidates(&requests[0]), 3_000_000)
        .unwrap();
    assert_ne!(fresh.run.id, entered.run.id);
}

#[test]
fn progress_reaches_the_directory_and_can_rebind_the_run_after_a_restart() {
    let (mut public, channel) = public_zone(&[(1, "aria", true)]);
    tick(&mut public);
    let requests = channel.take_requests();
    let mut directory = directory();
    let entered = directory.enter(&candidates(&requests[0]), 0).unwrap();

    // The directory's copy of the progress stands in for a report from the slot.
    directory.report(entered.run.slot, entered.run.id, 0b001, true);
    let run = directory.run_in_slot(entered.run.slot).unwrap().clone();
    let rebuilt = DepthsRunStart {
        run_id: run.id,
        roster: run.roster,
        cleared: run.cleared,
        checkpoint: run.checkpoint,
    };
    let mut slot = slot_adapter(entered.run.slot);
    let transfer = transfer_of(&mut public, 1, "aria", entered.run.slot);
    arrive(&mut slot, 1, &transfer, &rebuilt).unwrap();
    assert_eq!(slot.zone().depths_run().unwrap().cleared, 1);
    // The cleared chamber's creatures never stood again in this new binding.
    let standing: BTreeMap<_, _> = slot
        .zone()
        .snapshot()
        .unwrap()
        .creatures
        .iter()
        .map(|creature| (creature.creature_id.get(), creature.life))
        .collect();
    assert!(
        (1..=6).all(|id| standing[&id] != mmorpg_core::CreatureLife::Alive),
        "{standing:?}"
    );
}
