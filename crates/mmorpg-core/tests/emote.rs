//! Zone emotes (#69) through the public zone API: the `/say` range, the rate
//! limit shared with chat, ordering under the shared per-tick bound and
//! canonical continuation of pending and heard emotes.
mod support;

use std::sync::Arc;

use mmorpg_core::{
    CHAT_INTERVAL_TICKS, ChatChannel, ChatLine, ChatMessage, ChatText, Emote, ErrorCode,
    MAX_CHAT_PER_TICK, SAY_RANGE_UNITS, ZoneCommand, ZoneEvent, ZoneId, ZoneSimulation,
};
use support::arena;

fn say(line: &str) -> ZoneCommand {
    ZoneCommand::Chat {
        channel: ChatChannel::Say,
        text: ChatText::new(line).unwrap(),
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

fn heard(zone: &ZoneSimulation, player: u32) -> Vec<ChatLine> {
    zone.snapshot_for_player(player).unwrap().chat
}

fn throttled() -> [ZoneEvent; 1] {
    [ZoneEvent::Error {
        code: ErrorCode::ChatThrottled,
        target: None,
    }]
}

#[test]
fn an_emote_reaches_the_say_range_and_the_speaker() {
    let mut zone = crowd(&[[0, 0], [SAY_RANGE_UNITS, 0], [0, SAY_RANGE_UNITS + 1]]);
    zone.apply_command(1, 1, ZoneCommand::Emote(Emote::Wave))
        .unwrap();
    zone.advance_tick().unwrap();

    let wave = ChatLine {
        speaker: 1,
        message: ChatMessage::Emote(Emote::Wave),
    };
    assert_eq!(heard(&zone, 1), [wave]);
    assert_eq!(heard(&zone, 2), [wave]);
    assert!(heard(&zone, 3).is_empty());
    // Heard emotes last one tick, like chat lines.
    zone.advance_tick().unwrap();
    assert!(heard(&zone, 2).is_empty());
}

#[test]
fn emotes_share_the_speakers_rate_limit_with_chat() {
    let mut zone = crowd(&[[0, 0], [100, 0]]);
    zone.apply_command(1, 1, ZoneCommand::Emote(Emote::Bow))
        .unwrap();
    zone.apply_command(1, 2, say("too soon")).unwrap();
    zone.advance_tick().unwrap();
    assert_eq!(
        heard(&zone, 2),
        [ChatLine {
            speaker: 1,
            message: ChatMessage::Emote(Emote::Bow)
        }]
    );
    assert_eq!(zone.snapshot_for_player(1).unwrap().events, throttled());

    for _ in 1..CHAT_INTERVAL_TICKS {
        zone.advance_tick().unwrap();
    }
    zone.apply_command(1, 3, say("now")).unwrap();
    zone.apply_command(1, 4, ZoneCommand::Emote(Emote::Cheer))
        .unwrap();
    zone.advance_tick().unwrap();
    assert_eq!(heard(&zone, 2).len(), 1);
    assert_eq!(zone.snapshot_for_player(1).unwrap().events, throttled());
}

#[test]
fn emotes_and_lines_share_the_per_tick_bound_in_player_order() {
    let positions: Vec<[i32; 2]> = (0..6).map(|index| [index * 100, 0]).collect();
    let mut zone = crowd(&positions);
    for speaker in (1..=6).rev() {
        let command = if speaker % 2 == 0 {
            ZoneCommand::Emote(Emote::Laugh)
        } else {
            say("hello")
        };
        zone.apply_command(speaker, 1, command).unwrap();
    }
    zone.advance_tick().unwrap();

    let lines = heard(&zone, 6);
    assert_eq!(lines.len(), MAX_CHAT_PER_TICK);
    assert_eq!(
        lines.iter().map(|line| line.speaker).collect::<Vec<_>>(),
        [1, 2, 3, 4]
    );
    assert_eq!(lines[1].message, ChatMessage::Emote(Emote::Laugh));
}

#[test]
fn pending_and_heard_emotes_continue_after_recovery() {
    let mut original = crowd(&[[0, 0], [100, 0]]);
    original
        .apply_command(1, 1, ZoneCommand::Emote(Emote::Point))
        .unwrap();
    original.advance_tick().unwrap();
    original
        .apply_command(2, 1, ZoneCommand::Emote(Emote::Wave))
        .unwrap();
    let state = original.snapshot().unwrap();
    assert_eq!(
        state.players[1].chat,
        [ChatLine {
            speaker: 1,
            message: ChatMessage::Emote(Emote::Point)
        }]
    );

    let mut recovered =
        ZoneSimulation::from_snapshot(state, Arc::clone(original.content())).unwrap();
    for _ in 0..3 {
        original.advance_tick().unwrap();
        recovered.advance_tick().unwrap();
        assert_eq!(recovered.snapshot().unwrap(), original.snapshot().unwrap());
    }
}
