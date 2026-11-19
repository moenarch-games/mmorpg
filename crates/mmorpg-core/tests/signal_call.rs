//! The Rimewood signal post's horn call (#952): a Rogue pulls the signaler and his crossbowman.
//! Gouged during the committed call, or killed before it completes, the signaler brings nobody;
//! left alone, his call brings exactly his one authored partner from up the slope. The same
//! commands replay to the same zone.
mod support;

use mmorpg_core::ability::ids;
use mmorpg_core::greyhaven_vale::{self, signal_post};
use mmorpg_core::{
    CanonicalZoneSnapshot, CreatureAi, CreatureId, EntityRef, ZoneCommand, ZoneEvent, ZoneId,
    ZoneSimulation,
};
use support::{arena, classes};

const PLAYER: u32 = 1;
const ROGUE: u8 = 4;
/// Long enough for any call to have completed and its partner to have arrived: 8 s.
const AFTERMATH_TICKS: usize = 8 * mmorpg_core::TICK_HZ as usize;

/// How the Rogue answers the call.
#[derive(Clone, Copy, Debug)]
enum Answer {
    Gouge,
    Nothing,
    /// The signaler is already down to his last hit point, and a Sinister Strike finishes him.
    Kill,
}

/// A level 7 Rogue 2.5 m east of the signaler, inside his aggro radius.
fn start(answer: Answer) -> ZoneSimulation {
    let content = greyhaven_vale::content();
    let mut zone = ZoneSimulation::with_content(ZoneId::new(1), content).unwrap();
    zone.add_player(PLAYER).unwrap();
    let zone = classes::at_level(zone, PLAYER, 7);
    let mut state = zone.snapshot().unwrap();
    let [x, z] = signal_post::SIGNALER_AT;
    let x = x + 250;
    let ground = zone.content().definition().ground_at(x, z);
    state.players[0].position = mmorpg_core::physics::from_units([x, ground + 100, z]);
    if let Answer::Kill = answer {
        let signaler = state
            .creatures
            .iter_mut()
            .find(|c| c.creature_id == signal_post::SIGNALER)
            .unwrap();
        signaler.health = 1;
    }
    ZoneSimulation::from_snapshot(state, std::sync::Arc::clone(zone.content())).unwrap()
}

/// The zone, every command it was given by tick and every event the player saw.
struct Journey {
    zone: ZoneSimulation,
    commands: Vec<(usize, ZoneCommand)>,
    events: Vec<Vec<ZoneEvent>>,
    sequence: u32,
}

impl Journey {
    fn tick(&mut self, command: Option<ZoneCommand>) -> &[ZoneEvent] {
        if let Some(command) = command {
            self.sequence += 1;
            self.commands.push((self.events.len(), command));
            self.zone
                .apply_command(PLAYER, self.sequence, command)
                .unwrap();
        }
        self.zone.advance_tick().unwrap();
        let events = self.zone.snapshot_for_player(PLAYER).unwrap().events;
        self.events.push(events);
        self.events.last().unwrap()
    }

    /// Ticks until `done`, at most `limit` ticks.
    fn until(&mut self, limit: usize, done: impl Fn(&CanonicalZoneSnapshot) -> bool) {
        for _ in 0..limit {
            if done(&self.zone.snapshot().unwrap()) {
                return;
            }
            self.tick(None);
        }
        panic!("not reached in {limit} ticks");
    }

    fn state(&self) -> CanonicalZoneSnapshot {
        self.zone.snapshot().unwrap()
    }
}

fn ai(state: &CanonicalZoneSnapshot, id: CreatureId) -> CreatureAi {
    state
        .creatures
        .iter()
        .find(|c| c.creature_id == id)
        .unwrap()
        .ai
}

fn calling(state: &CanonicalZoneSnapshot) -> bool {
    state
        .creatures
        .iter()
        .find(|c| c.creature_id == signal_post::SIGNALER)
        .and_then(|c| c.abilities.cast)
        .is_some_and(|cast| cast.ability == ids::SIGNAL_HORN)
}

/// Every engaged creature in the zone, by ID.
fn engaged(state: &CanonicalZoneSnapshot) -> Vec<CreatureId> {
    state
        .creatures
        .iter()
        .filter(|c| c.ai == CreatureAi::Engaged)
        .map(|c| c.creature_id)
        .collect()
}

fn journey(answer: Answer) -> Journey {
    let mut j = Journey {
        zone: start(answer),
        commands: Vec::new(),
        events: Vec::new(),
        sequence: 0,
    };
    let signaler = EntityRef::Creature(signal_post::SIGNALER);
    j.tick(Some(ZoneCommand::ChooseClass {
        class: ROGUE,
        sex: 0,
    }));
    // The signaler notices the Rogue and, standing still, raises his horn.
    j.until(200, calling);
    let state = j.state();
    assert!(matches!(
        ai(&state, signal_post::PARTNER),
        CreatureAi::Idle { .. }
    ));
    let ability = match answer {
        Answer::Gouge => Some(ids::GOUGE),
        Answer::Kill => Some(ids::SINISTER_STRIKE),
        Answer::Nothing => None,
    };
    if let Some(ability) = ability {
        let (facing, _) = arena::aim_at(&j.zone, PLAYER, signaler).unwrap();
        j.tick(Some(arena::stand(facing)));
        j.tick(Some(ZoneCommand::UseAbility {
            ability: ability.get(),
            target: Some(signaler),
            facing: Some(facing),
        }));
    }
    let mut called = false;
    for _ in 0..AFTERMATH_TICKS {
        let events = j.tick(None);
        called |= events.iter().any(|event| {
            matches!(*event, ZoneEvent::AbilityUsed { source, ability, .. }
                if source == signaler && ability == ids::SIGNAL_HORN)
        });
    }
    let state = j.state();
    let partner = ai(&state, signal_post::PARTNER);
    let mut expected = vec![signal_post::SIGNALER, signal_post::CROSSBOWMAN];
    match answer {
        Answer::Gouge => {
            let interrupted = j.events.iter().flatten().any(|event| {
                matches!(*event, ZoneEvent::Interrupted { target, ability, .. }
                    if target == signaler && ability == ids::SIGNAL_HORN)
            });
            assert!(interrupted, "the Gouge breaks the call");
            assert!(!called, "no call completes");
            assert!(matches!(partner, CreatureAi::Idle { .. }), "{partner:?}");
        }
        Answer::Kill => {
            expected.retain(|&id| id != signal_post::SIGNALER);
            assert!(!called, "no call completes");
            assert!(matches!(partner, CreatureAi::Idle { .. }), "{partner:?}");
        }
        Answer::Nothing => {
            assert!(called, "the call completes");
            assert_eq!(partner, CreatureAi::Engaged, "the partner joins");
            expected.push(signal_post::PARTNER);
        }
    }
    assert_eq!(engaged(&state), expected, "exactly the post fights");
    j
}

/// The same commands on the same ticks give the same zone and the same events.
fn replays(played: &Journey, answer: Answer) {
    let mut replay = Journey {
        zone: start(answer),
        commands: Vec::new(),
        events: Vec::new(),
        sequence: 0,
    };
    let mut commands = played.commands.iter().peekable();
    for tick in 0..played.events.len() {
        let command = commands
            .next_if(|(at, _)| *at == tick)
            .map(|&(_, command)| command);
        replay.tick(command);
    }
    assert_eq!(replay.events, played.events);
    assert_eq!(replay.state(), played.state());
}

#[test]
fn the_signalers_call_brings_one_partner_unless_interrupted_or_cut_short_and_replays_exactly() {
    for answer in [Answer::Gouge, Answer::Nothing, Answer::Kill] {
        let played = journey(answer);
        replays(&played, answer);
    }
}
