//! Buffered guard after a directed strike (#156, Atlas 5.2.39, 5.2.41): the
//! strike commits the Warden to a short recovery; a guard pressed during or
//! right after it is held but protects only when the recovery ends, and a
//! release before then discards it.
mod support;

use std::sync::Arc;

use mmorpg_core::ability::ids;
use mmorpg_core::{
    STRIKE_RECOVERY_TICKS, StrikeAim, StrikeHeight, StrikeSide, ZoneAreas, ZoneCommand,
    ZoneContent, ZoneId, ZoneSimulation,
};
use support::arena::{self, GRAVEYARD};

const SOUTH: u16 = 32_768;

struct Warden {
    zone: ZoneSimulation,
    sequence: u32,
}

impl Warden {
    /// A level-1 Warden on flat ground with full Rage.
    fn new() -> Self {
        let content = Arc::new(
            ZoneContent::new(
                arena::ground(),
                ZoneAreas::default(),
                Vec::new(),
                Vec::new(),
                Vec::new(),
                GRAVEYARD,
            )
            .unwrap()
            .with_creature_abilities(Default::default())
            .unwrap(),
        );
        let mut zone = ZoneSimulation::with_content(ZoneId::new(9), Arc::clone(&content)).unwrap();
        zone.add_player(1).unwrap();
        let mut warden = Self { zone, sequence: 0 };
        warden.send(ZoneCommand::ChooseClass { class: 0, sex: 0 });
        warden.send(arena::stand(SOUTH));
        for _ in 0..20 {
            warden.zone.advance_tick().unwrap();
        }
        let mut state = warden.zone.snapshot().unwrap();
        state.players[0].combat.abilities.resource = 100;
        warden.zone = ZoneSimulation::from_snapshot(state, content).unwrap();
        warden
    }

    fn send(&mut self, command: ZoneCommand) {
        self.sequence += 1;
        self.zone.apply_command(1, self.sequence, command).unwrap();
    }

    fn slash(&mut self) {
        self.send(ZoneCommand::DirectedStrike {
            ability: ids::HEROIC_STRIKE.get(),
            facing: SOUTH,
            aim: StrikeAim::new(StrikeSide::Centre, StrikeHeight::Middle),
        });
    }

    fn guard(&mut self) -> mmorpg_core::GuardView {
        self.zone.advance_tick().unwrap();
        self.zone.snapshot_for_player(1).unwrap().viewer.guard
    }
}

#[test]
fn a_guard_pressed_after_a_strike_protects_once_its_recovery_ends() {
    let mut warden = Warden::new();
    warden.slash();
    // Pressed right after the strike, in the same queue: the last press wins.
    warden.send(ZoneCommand::Guard {
        lane: Some(StrikeHeight::High),
    });
    let mut waited = 0;
    loop {
        let guard = warden.guard();
        assert_eq!(guard.raised, Some(StrikeHeight::High), "the press is held");
        if guard.protecting {
            break;
        }
        waited += 1;
        assert!(
            waited <= STRIKE_RECOVERY_TICKS,
            "protection begins after the recovery"
        );
    }
    assert!(waited > 0, "the strike's recovery delays protection");
}

#[test]
fn a_guard_released_before_the_recovery_ends_never_protects() {
    let mut warden = Warden::new();
    warden.slash();
    warden.send(ZoneCommand::Guard {
        lane: Some(StrikeHeight::Low),
    });
    assert!(!warden.guard().protecting);
    warden.send(ZoneCommand::Guard { lane: None });
    for _ in 0..2 * STRIKE_RECOVERY_TICKS {
        let guard = warden.guard();
        assert_eq!(guard.raised, None);
        assert!(!guard.protecting);
    }
}

#[test]
fn a_strike_pressed_after_the_guard_lowers_it() {
    let mut warden = Warden::new();
    warden.send(ZoneCommand::Guard {
        lane: Some(StrikeHeight::Middle),
    });
    assert!(warden.guard().protecting);
    warden.slash();
    assert_eq!(warden.guard().raised, None);
}
