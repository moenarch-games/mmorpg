//! Latency-tolerant, zone-authoritative combo timing (#415, Atlas 5.9.6).
//!
//! An online command may carry the completed zone tick its client had last
//! observed. The zone lets the Warden's slash → Shield Bash edge make up for
//! that lateness, up to `action::LATE_INPUT_GRACE_MS`, and resolves the
//! follow-up in its arrival tick against the current world. Observations
//! ahead of the zone or older than the previous one fail closed.
mod support;

use std::sync::Arc;

use mmorpg_core::ability::ids;
use mmorpg_core::action::{self, ActionInput, BlockReason, Candidate, EdgeId, EdgeStatus, Outcome};
use mmorpg_core::{
    CreatureBehaviour, CreatureTemplate, EntityRef, StrikeAim, StrikeHeight, StrikeSide, ZoneAreas,
    ZoneCommand, ZoneContent, ZoneEvent, ZoneId, ZoneSimulation,
};
use support::arena::{self, GRAVEYARD, WOLF};

const SOUTH: u16 = 32_768;
const GRACE: u64 = action::LATE_INPUT_GRACE_TICKS as u64;

fn content() -> Arc<ZoneContent> {
    let template = CreatureTemplate {
        behaviour: CreatureBehaviour::Neutral,
        ..arena::wolf(5_000, [1, 1])
    };
    Arc::new(
        ZoneContent::new(
            arena::ground(),
            ZoneAreas::default(),
            vec![template],
            vec![arena::spawn(1, WOLF, [0, -150])],
            Vec::new(),
            GRAVEYARD,
        )
        .unwrap()
        .with_creature_abilities(Default::default())
        .unwrap(),
    )
}

struct Warden {
    zone: ZoneSimulation,
    content: Arc<ZoneContent>,
    sequence: u32,
}

impl Warden {
    /// A level-2 Warden (Shield Bash learned) with full Rage facing the wolf.
    fn new() -> Self {
        let content = content();
        let mut zone = ZoneSimulation::with_content(ZoneId::new(9), Arc::clone(&content)).unwrap();
        zone.add_player(1).unwrap();
        let mut warden = Self {
            zone,
            content,
            sequence: 0,
        };
        warden.send(ZoneCommand::ChooseClass { class: 0, sex: 0 });
        warden.send(arena::stand(SOUTH));
        for _ in 0..mmorpg_core::golden::t30(20) {
            warden.zone.advance_tick().unwrap();
        }
        let mut state = warden.zone.snapshot().unwrap();
        let player = &mut state.players[0].combat;
        player.level = 2;
        player.health = mmorpg_core::unit::player_max_health(2);
        player.abilities.resource = 100;
        warden.zone = ZoneSimulation::from_snapshot(state, Arc::clone(&warden.content)).unwrap();
        warden
    }

    fn tick(&self) -> u64 {
        self.zone.current_tick()
    }

    fn send(&mut self, command: ZoneCommand) {
        self.sequence += 1;
        self.zone.apply_command(1, self.sequence, command).unwrap();
    }

    fn send_observed(&mut self, command: ZoneCommand, observed: u64) {
        self.sequence += 1;
        self.zone
            .apply_observed_command(1, self.sequence, command, observed)
            .unwrap();
    }

    fn step(&mut self) -> Vec<ZoneEvent> {
        self.zone.advance_tick().unwrap();
        self.zone.snapshot_for_player(1).unwrap().events
    }

    /// A centred directed slash into the wolf.
    fn slash(&mut self) {
        self.send(ZoneCommand::DirectedStrike {
            ability: ids::HEROIC_STRIKE.get(),
            facing: SOUTH,
            aim: StrikeAim::new(StrikeSide::Centre, StrikeHeight::Middle),
        });
        self.step();
        assert!(
            self.zone.snapshot().unwrap().players[0]
                .combat
                .combo
                .is_some(),
            "the slash confirms"
        );
    }

    /// The last tick whose intents still resolve inside the open window.
    fn open_until(&self) -> u64 {
        self.zone.action_inspection(1).unwrap().edges[0]
            .open_until
            .unwrap()
    }

    /// Steps until the next tick resolves intents at `tick`.
    fn until_resolving(&mut self, tick: u64) {
        while self.tick() + 1 < tick {
            self.step();
        }
    }

    /// A Shield Bash without a selected target: only the combo's confirmed
    /// target lets it hit.
    fn bash() -> ZoneCommand {
        ZoneCommand::UseAbility {
            ability: ids::SHIELD_BASH.get(),
            target: None,
        }
    }

    /// Whether the Bash resolving in the next tick follows up on the wolf.
    fn bash_follows_up(&mut self) -> bool {
        self.step().iter().any(|event| {
            matches!(
                event,
                ZoneEvent::AbilityUsed { ability, target: Some(EntityRef::Creature(_)), .. }
                    if *ability == ids::SHIELD_BASH
            )
        })
    }
}

#[test]
fn a_late_bash_catches_the_window_it_observed_open() {
    let mut warden = Warden::new();
    warden.slash();
    let close = warden.open_until();
    // The Bash resolves four ticks after the window's last actionable tick,
    // sent while its client saw a world five ticks older.
    warden.until_resolving(close + 4);
    let observed = warden.tick() - 5;
    warden.send_observed(Warden::bash(), observed);
    let arbitration = warden
        .zone
        .arbitrate_late_input(1, ActionInput::ShieldBash, 5)
        .unwrap();
    assert_eq!(
        arbitration.outcome,
        Outcome::Proceed(Candidate::Edge(EdgeId::SlashToShieldBash))
    );
    assert!(arbitration.compensated);
    assert_eq!(arbitration.late, 5);
    // A punctual input sees the window gone.
    assert_eq!(
        warden.zone.action_inspection(1).unwrap().edges[0].status,
        EdgeStatus::Blocked(BlockReason::SourceInactive)
    );
    assert!(warden.bash_follows_up(), "the zone accepts the follow-up");
}

#[test]
fn the_same_bash_without_enough_lateness_is_refused() {
    for observed_lateness in [None, Some(0), Some(3)] {
        let mut warden = Warden::new();
        warden.slash();
        let close = warden.open_until();
        warden.until_resolving(close + 4);
        match observed_lateness {
            Some(late) => warden.send_observed(Warden::bash(), warden.tick() - late),
            None => warden.send(Warden::bash()),
        }
        assert!(!warden.bash_follows_up(), "{observed_lateness:?}");
    }
}

#[test]
fn lateness_beyond_the_grace_never_counts() {
    let mut warden = Warden::new();
    warden.slash();
    let close = warden.open_until();
    // One tick past the grace: even an ancient observation is capped.
    warden.until_resolving(close + GRACE + 1);
    warden.send_observed(Warden::bash(), 0);
    assert!(
        warden.zone.snapshot().unwrap().players[0].combat.intents[0].late
            == action::LATE_INPUT_GRACE_TICKS
    );
    assert!(!warden.bash_follows_up());

    // At exactly the grace it still counts.
    let mut warden = Warden::new();
    warden.slash();
    let close = warden.open_until();
    warden.until_resolving(close + GRACE);
    warden.send_observed(Warden::bash(), 0);
    assert!(warden.bash_follows_up());
}

#[test]
fn a_late_bash_inside_the_window_resolves_as_before() {
    let mut warden = Warden::new();
    warden.slash();
    let close = warden.open_until();
    warden.until_resolving(close - 2);
    let observed = warden.tick() - 4;
    warden.send_observed(Warden::bash(), observed);
    let arbitration = warden
        .zone
        .arbitrate_late_input(1, ActionInput::ShieldBash, 4)
        .unwrap();
    assert!(!arbitration.compensated, "the window is still open");
    assert!(warden.bash_follows_up());
}

#[test]
fn observations_ahead_of_the_zone_or_older_than_the_last_fail_closed() {
    let mut warden = Warden::new();
    let now = warden.tick();
    let sequence = warden.sequence + 1;
    assert!(
        warden
            .zone
            .apply_observed_command(1, sequence, Warden::bash(), now + 1)
            .is_err()
    );
    warden
        .zone
        .apply_observed_command(1, sequence, ZoneCommand::Jump, now - 2)
        .unwrap();
    assert!(
        warden
            .zone
            .apply_observed_command(1, sequence + 1, Warden::bash(), now - 3)
            .is_err(),
        "an observation may not go back"
    );
    // The refused commands consumed nothing.
    warden
        .zone
        .apply_observed_command(1, sequence + 1, ZoneCommand::Jump, now - 2)
        .unwrap();
    let state = warden.zone.snapshot().unwrap();
    assert_eq!(state.players[0].last_sequence, sequence + 1);
    assert_eq!(state.players[0].combat.last_observed, now - 2);
}

#[test]
fn a_late_intent_survives_a_checkpoint_before_its_tick() {
    let mut warden = Warden::new();
    warden.slash();
    let close = warden.open_until();
    warden.until_resolving(close + 2);
    let observed = warden.tick() - 3;
    warden.send_observed(Warden::bash(), observed);
    let checkpoint = warden.zone.snapshot().unwrap();
    assert_eq!(checkpoint.players[0].combat.intents[0].late, 3);
    warden.zone = ZoneSimulation::from_snapshot(checkpoint, Arc::clone(&warden.content)).unwrap();
    assert!(warden.bash_follows_up());
}

/// The combo edge with a 100 ms sweet spot at its opening.
static PRECISE_EDGES: [action::EdgeDef; 1] = [action::EdgeDef {
    precision: Some(action::Precision {
        after_open_ms: 0,
        length_ms: 100,
        benefits: &[action::Effect::ResourceRefund { amount: 5 }],
    }),
    ..action::EDGES[0]
}];
static PRECISE: action::ActionGraph = action::ActionGraph {
    edges: &PRECISE_EDGES,
    ..action::BASELINE
};

#[test]
fn lateness_never_makes_an_input_precise() {
    let mut warden = Warden::new();
    warden.zone.set_action_graph(&PRECISE).unwrap();
    warden.slash();
    let edge = warden.zone.action_inspection(1).unwrap().edges[0];
    let (_, sweet_until) = edge.precision.unwrap();
    // Arriving three ticks after the sweet spot, sent while it was running.
    warden.until_resolving(sweet_until + 3);
    let late = warden
        .zone
        .arbitrate_late_input(1, ActionInput::ShieldBash, 6)
        .unwrap();
    assert_eq!(
        late.outcome,
        Outcome::Proceed(Candidate::Edge(EdgeId::SlashToShieldBash))
    );
    assert!(!late.precise, "precision counts only on arrival (#415 Q2)");

    // A late input that the grace keeps in the window is never precise either.
    let close = warden.open_until();
    warden.until_resolving(close + 2);
    let rescued = warden
        .zone
        .arbitrate_late_input(1, ActionInput::ShieldBash, action::LATE_INPUT_GRACE_TICKS)
        .unwrap();
    assert!(rescued.compensated && !rescued.precise);

    // Inside the sweet spot on arrival, it is precise as before.
    let mut warden = Warden::new();
    warden.zone.set_action_graph(&PRECISE).unwrap();
    warden.slash();
    let (sweet_from, _) = warden.zone.action_inspection(1).unwrap().edges[0]
        .precision
        .unwrap();
    warden.until_resolving(sweet_from + 1);
    assert!(
        warden
            .zone
            .arbitrate_late_input(1, ActionInput::ShieldBash, 4)
            .unwrap()
            .precise
    );
}
