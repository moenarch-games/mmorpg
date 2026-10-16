//! Zone chat through the public zone API: say and yell ranges, the
//! once-per-second rate limit, the per-tick bound, ordering and canonical
//! continuation of pending lines and the rate limit.
mod support;

use std::sync::Arc;

use mmorpg_core::{
    CHAT_INTERVAL_TICKS, ChatChannel, ChatLine, ChatMessage, ChatText, ErrorCode,
    MAX_CHAT_PER_TICK, SAY_RANGE_UNITS, YELL_RANGE_UNITS, ZoneCommand, ZoneEvent, ZoneId,
    ZoneSimulation,
};
use support::arena;

fn text(line: &str) -> ChatText {
    ChatText::new(line).unwrap()
}

fn say(line: &str) -> ZoneCommand {
    ZoneCommand::Chat {
        channel: ChatChannel::Say,
        text: text(line),
    }
}

fn yell(line: &str) -> ZoneCommand {
    ZoneCommand::Chat {
        channel: ChatChannel::Yell,
        text: text(line),
    }
}

/// Players 1..=`count` restored at the given XZ positions on flat ground.
fn crowd(positions: &[[i32; 2]]) -> ZoneSimulation {
    let mut zone =
        ZoneSimulation::with_content(ZoneId::new(1), arena::arena(vec![], vec![], vec![])).unwrap();
    for id in 1..=positions.len() {
        zone.add_player(u32::try_from(id).unwrap()).unwrap();
    }
    let mut state = zone.snapshot().unwrap();
    for (player, position) in state.players.iter_mut().zip(positions) {
        player.position = [position[0], 90, position[1]];
    }
    ZoneSimulation::from_snapshot(state, Arc::clone(zone.content())).unwrap()
}

fn heard(zone: &ZoneSimulation, player: u32) -> Vec<(u32, String)> {
    zone.snapshot_for_player(player)
        .unwrap()
        .chat
        .iter()
        .map(|line| match line.message {
            ChatMessage::Say(text) | ChatMessage::Yell(text) => {
                (line.speaker, text.as_str().to_owned())
            }
            ChatMessage::Emote(emote) => (line.speaker, format!("*{emote:?}*")),
        })
        .collect()
}

#[test]
fn say_and_yell_reach_their_inclusive_ranges_and_the_speaker() {
    let mut zone = crowd(&[
        [0, 0],
        [SAY_RANGE_UNITS, 0],
        [0, SAY_RANGE_UNITS + 1],
        [YELL_RANGE_UNITS, 0],
        [-YELL_RANGE_UNITS - 1, 0],
    ]);
    zone.apply_command(1, 1, say("Hail")).unwrap();
    zone.advance_tick().unwrap();
    let hail = vec![(1, "Hail".to_owned())];
    assert_eq!(heard(&zone, 1), hail);
    assert_eq!(heard(&zone, 2), hail);
    for out_of_range in [3, 4, 5] {
        assert!(heard(&zone, out_of_range).is_empty());
    }
    // Lines last one tick, like events.
    zone.advance_tick().unwrap();
    assert!(heard(&zone, 2).is_empty());
    zone.apply_command(1, 2, yell("To arms")).unwrap();
    for _ in 0..CHAT_INTERVAL_TICKS {
        zone.advance_tick().unwrap();
    }
    let yelled = zone.snapshot_for_player(4).unwrap().chat;
    assert!(
        yelled.is_empty(),
        "the yell was refused within the interval"
    );
    zone.apply_command(1, 3, yell("To arms")).unwrap();
    zone.advance_tick().unwrap();
    for hearer in [1, 2, 3, 4] {
        assert_eq!(heard(&zone, hearer), vec![(1, "To arms".to_owned())]);
    }
    assert!(heard(&zone, 5).is_empty());
    assert_eq!(
        zone.snapshot_for_player(4).unwrap().chat[0].message,
        ChatMessage::Yell(text("To arms"))
    );
}

#[test]
fn a_speaker_may_speak_once_per_second_and_refusals_are_events() {
    let mut zone = crowd(&[[0, 0], [100, 0]]);
    zone.apply_command(1, 1, say("one")).unwrap();
    zone.apply_command(1, 2, say("two")).unwrap();
    zone.advance_tick().unwrap();
    assert_eq!(heard(&zone, 2), vec![(1, "one".to_owned())]);
    assert_eq!(
        zone.snapshot_for_player(1).unwrap().events,
        [ZoneEvent::Error {
            code: ErrorCode::ChatThrottled,
            target: None
        }]
    );
    for _ in 1..CHAT_INTERVAL_TICKS - 1 {
        zone.advance_tick().unwrap();
    }
    zone.apply_command(1, 3, say("early")).unwrap();
    zone.advance_tick().unwrap();
    assert!(heard(&zone, 2).is_empty());
    zone.apply_command(1, 4, say("now")).unwrap();
    zone.advance_tick().unwrap();
    assert_eq!(heard(&zone, 2), vec![(1, "now".to_owned())]);
    // Stale and duplicate sequences fail closed and say nothing.
    assert!(zone.apply_command(1, 4, say("again")).is_err());
    assert!(zone.apply_command(1, 3, say("again")).is_err());
}

#[test]
fn each_player_hears_at_most_four_lines_per_tick_in_player_order() {
    let positions: Vec<[i32; 2]> = (0..6).map(|index| [index * 100, 0]).collect();
    let mut zone = crowd(&positions);
    for speaker in (1..=6).rev() {
        zone.apply_command(speaker, 1, say(&format!("from {speaker}")))
            .unwrap();
    }
    zone.advance_tick().unwrap();
    let lines = zone.snapshot_for_player(6).unwrap().chat;
    assert_eq!(lines.len(), MAX_CHAT_PER_TICK);
    assert_eq!(
        lines.iter().map(|line| line.speaker).collect::<Vec<_>>(),
        [1, 2, 3, 4]
    );
    // The later speakers still spoke: their rate limit started.
    zone.apply_command(6, 2, say("again")).unwrap();
    zone.advance_tick().unwrap();
    assert_eq!(
        zone.snapshot_for_player(6).unwrap().events,
        [ZoneEvent::Error {
            code: ErrorCode::ChatThrottled,
            target: None
        }]
    );
}

#[test]
fn pending_lines_heard_lines_and_the_rate_limit_continue_after_recovery() {
    let mut original = crowd(&[[0, 0], [100, 0]]);
    original.apply_command(1, 1, say("before")).unwrap();
    original.advance_tick().unwrap();
    original.apply_command(2, 1, yell("queued")).unwrap();
    original.apply_command(1, 2, say("throttled")).unwrap();
    let state = original.snapshot().unwrap();
    assert_eq!(state.players[0].chat.len(), 1);
    assert_eq!(
        state.players[0].chat[0],
        ChatLine {
            speaker: 1,
            message: ChatMessage::Say(text("before"))
        }
    );
    let mut recovered =
        ZoneSimulation::from_snapshot(state.clone(), Arc::clone(original.content())).unwrap();
    assert_eq!(
        recovered.snapshot_for_player(2).unwrap().chat,
        original.snapshot_for_player(2).unwrap().chat
    );
    for _ in 0..3 {
        original.advance_tick().unwrap();
        recovered.advance_tick().unwrap();
        assert_eq!(recovered.snapshot().unwrap(), original.snapshot().unwrap());
    }
    // Recovery refuses chat state beyond its bounds.
    let mut flooded = state.clone();
    flooded.players[0].chat = vec![flooded.players[0].chat[0]; MAX_CHAT_PER_TICK + 1];
    assert!(ZoneSimulation::from_snapshot(flooded, Arc::clone(original.content())).is_err());
    let mut future = state;
    future.players[0].chat_ready_at = future.tick + CHAT_INTERVAL_TICKS + 1;
    assert!(ZoneSimulation::from_snapshot(future, Arc::clone(original.content())).is_err());
}
