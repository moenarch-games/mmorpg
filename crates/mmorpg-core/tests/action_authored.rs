//! Authored actions (#414, Atlas 5.9.4–5.9.5): an experimental graph
//! composes a new named action from existing primitives (a contextual
//! trigger, timed phases with bounded movement, typed effects on the
//! ordinary use and outgoing edges), and the zone runs it through the
//! ordinary intent, edge and movement code.
mod support;

use std::sync::Arc;

use mmorpg_core::ability::ids;
use mmorpg_core::action::{
    self, ActionGraph, ActionId, ActionInput, Anchor, AuthoredAction, AuthoredPhase,
    AuthoredProblem, Direction, EdgeDef, EdgeId, Effect, GraphError, InputContext, Movement,
    Origin, Outcome, Phase, Region, Trigger, TriggerRefusal, TriggerStatus, Window,
};
use mmorpg_core::{
    CreatureBehaviour, CreatureId, EntityRef, PlayerClass, StrikeAim, StrikeHeight, StrikeSide,
    ZoneAreas, ZoneCommand, ZoneContent, ZoneEvent, ZoneId, ZoneSimulation, ticks_from_ms,
};
use support::arena::{self, GRAVEYARD, WOLF};

const SOUTH: u16 = 32_768;

/// A Warden's sliding slash: Heroic Strike while moving forward winds up
/// for 100 ms, slides 300 ms on its momentum with a forward launch, then
/// recovers 200 ms at half steering.
const SLIDING_SLASH: AuthoredAction = AuthoredAction {
    slot: 0,
    name: "Sliding slash",
    class: PlayerClass::Warden,
    trigger: Trigger {
        input: ActionInput::WardenMelee,
        ability: Some(ids::HEROIC_STRIKE),
        context: InputContext::Moving(Direction::Forward),
    },
    conditions: &[],
    startup: AuthoredPhase::free(100),
    active: AuthoredPhase {
        ms: 300,
        movement: Movement {
            steering_percent: 0,
            momentum_percent: 100,
            max_acceleration_cms2: None,
        },
    },
    recovery: AuthoredPhase {
        ms: 200,
        movement: Movement {
            steering_percent: 50,
            momentum_percent: 0,
            max_acceleration_cms2: None,
        },
    },
    effects: &[Effect::Impulse {
        forward_cms: 400,
        up_cms: 0,
    }],
};

/// Out of the slash into Shield Bash 100 ms after its impact, for 200 ms,
/// waiving the global cooldown.
const SLIDE_TO_BASH: EdgeDef = EdgeDef {
    id: EdgeId::Authored(0),
    from: ActionId::Authored(0),
    input: ActionInput::ShieldBash,
    anchor: Anchor::Impact,
    window: Window {
        open_after_ms: 100,
        actionable_ms: 200,
        buffer_ms: 0,
    },
    conditions: &[],
    effects: &[Effect::IgnoreGlobalCooldown],
    precision: None,
    priority: 2,
};

static SLIDING: ActionGraph = ActionGraph {
    authored: &[SLIDING_SLASH],
    ..action::BASELINE
};

static SLIDING_WITH_BASH: ActionGraph = ActionGraph {
    edges: &[action::EDGES[0], SLIDE_TO_BASH],
    authored: &[SLIDING_SLASH],
    ..action::BASELINE
};

fn content() -> Arc<ZoneContent> {
    let template = mmorpg_core::CreatureTemplate {
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

struct Hero {
    zone: ZoneSimulation,
    sequence: u32,
}

impl Hero {
    fn new(class: PlayerClass, graph: &'static ActionGraph) -> Self {
        let content = content();
        let mut zone = ZoneSimulation::with_content(ZoneId::new(9), Arc::clone(&content)).unwrap();
        zone.add_player(1).unwrap();
        let mut hero = Self { zone, sequence: 0 };
        hero.send(ZoneCommand::ChooseClass {
            class: class.code(),
            sex: 0,
        });
        hero.send(arena::stand(SOUTH));
        hero.ticks(mmorpg_core::golden::t30(20));
        let mut state = hero.zone.snapshot().unwrap();
        let player = &mut state.players[0].combat;
        player.level = 2;
        player.health = mmorpg_core::unit::player_max_health(2);
        player.abilities.resource = 100;
        hero.zone = ZoneSimulation::from_snapshot(state, content).unwrap();
        hero.zone.set_action_graph(graph).unwrap();
        hero
    }

    fn send(&mut self, command: ZoneCommand) {
        self.sequence += 1;
        self.zone.apply_command(1, self.sequence, command).unwrap();
    }

    fn tick(&mut self) -> Vec<ZoneEvent> {
        self.zone.advance_tick().unwrap();
        self.zone.snapshot_for_player(1).unwrap().events
    }

    fn ticks(&mut self, count: u64) {
        for _ in 0..count {
            self.tick();
        }
    }

    fn inspect(&self) -> action::ActionInspection {
        self.zone.action_inspection(1).unwrap()
    }

    fn offense(&self) -> action::RegionReport {
        self.inspect().regions[Region::Offense as usize]
    }

    /// The running authored action's slot and phase.
    fn running(&self) -> Option<(u8, Phase)> {
        self.inspect()
            .authored
            .iter()
            .find_map(|report| match report.status {
                TriggerStatus::Running { phase, .. } => Some((report.slot, phase)),
                _ => None,
            })
    }

    fn strike(&mut self) {
        self.send(ZoneCommand::DirectedStrike {
            ability: ids::HEROIC_STRIKE.get(),
            facing: SOUTH,
            aim: StrikeAim::new(StrikeSide::Centre, StrikeHeight::Low),
        });
    }

    fn bash(&mut self) {
        self.send(ZoneCommand::UseAbility {
            ability: ids::SHIELD_BASH.get(),
            target: Some(EntityRef::Creature(CreatureId::new(1))),
        });
    }
}

fn swept(events: &[ZoneEvent]) -> bool {
    events
        .iter()
        .any(|event| matches!(event, ZoneEvent::StrikeSwept { .. }))
}

fn bashed(events: &[ZoneEvent]) -> bool {
    events.iter().any(|event| {
        matches!(event, ZoneEvent::AbilityUsed { ability, .. } if *ability == ids::SHIELD_BASH)
    })
}

#[test]
fn the_production_graph_authors_no_action() {
    assert!(action::BASELINE.authored.is_empty());
    let warden = Hero::new(PlayerClass::Warden, &action::BASELINE);
    assert!(warden.inspect().authored.is_empty());
}

#[test]
fn a_sliding_slash_winds_up_resolves_the_ordinary_strike_and_recovers() {
    let mut warden = Hero::new(PlayerClass::Warden, &SLIDING);
    // Standing still the context is missing: the strike is the ordinary one.
    let report = warden.inspect().authored[0];
    assert_eq!(report.name, "Sliding slash");
    assert_eq!(
        report.status,
        TriggerStatus::Refused(TriggerRefusal::Context(InputContext::Moving(
            Direction::Forward
        )))
    );

    warden.send(arena::walk(SOUTH));
    warden.tick();
    assert_eq!(warden.inspect().authored[0].status, TriggerStatus::Accepted);
    warden.strike();
    let events = warden.tick();
    assert!(!swept(&events), "the startup holds the strike");
    let offense = warden.offense();
    assert_eq!(offense.action, Some(ActionId::Authored(0)));
    assert_eq!(offense.phase, Some(Phase::Startup));
    assert!(matches!(
        warden.inspect().authored[0].status,
        TriggerStatus::Running {
            phase: Phase::Startup,
            ..
        }
    ));

    let startup = u64::from(ticks_from_ms(100));
    let mut resolved = None;
    for tick in 1..=startup {
        if swept(&warden.tick()) {
            resolved = Some(tick);
            break;
        }
    }
    assert_eq!(
        resolved,
        Some(startup),
        "the strike resolves as startup ends"
    );
    // The active phase slides on the momentum, as the authored action says.
    assert_eq!(warden.running(), Some((0, Phase::Active)));
    let inspection = warden.inspect();
    let source = inspection
        .movement
        .sources
        .iter()
        .find(|source| source.action == ActionId::Authored(0))
        .expect("the active phase moves as authored");
    assert_eq!(source.origin, Origin::Authored(0));
    assert_eq!(source.movement, SLIDING_SLASH.active.movement);

    // Shield Bash waits for the action to end.
    let bash = warden
        .zone
        .arbitrate_input(1, ActionInput::ShieldBash)
        .unwrap();
    assert_eq!(bash.outcome, Outcome::WaitForAuthored(0));

    warden.ticks(u64::from(ticks_from_ms(300)));
    assert_eq!(warden.running(), Some((0, Phase::Recovery)));
    warden.ticks(u64::from(ticks_from_ms(200)));
    assert_eq!(warden.running(), None);
    assert!(matches!(
        warden.inspect().authored[0].status,
        TriggerStatus::Accepted | TriggerStatus::Refused(_)
    ));
}

#[test]
fn the_launch_moves_the_slide_further_than_a_plain_strike() {
    let travel = |graph: &'static ActionGraph| {
        let mut warden = Hero::new(PlayerClass::Warden, graph);
        // Walk away from the wolf, north, and strike while moving.
        warden.send(arena::walk(0));
        warden.ticks(mmorpg_core::golden::t30(5));
        let from = warden.zone.snapshot().unwrap().players[0].position;
        warden.send(ZoneCommand::DirectedStrike {
            ability: ids::HEROIC_STRIKE.get(),
            facing: 0,
            aim: StrikeAim::new(StrikeSide::Centre, StrikeHeight::Low),
        });
        warden.ticks(u64::from(ticks_from_ms(400)));
        let to = warden.zone.snapshot().unwrap().players[0].position;
        to[2] - from[2]
    };
    let plain = travel(&action::BASELINE);
    let sliding = travel(&SLIDING);
    assert!(sliding > plain + 50.0, "{plain} → {sliding}");
}

#[test]
fn an_edge_out_of_the_slash_follows_into_shield_bash_and_ends_it() {
    let mut warden = Hero::new(PlayerClass::Warden, &SLIDING_WITH_BASH);
    warden.send(arena::walk(SOUTH));
    warden.tick();
    warden.strike();
    warden.ticks(u64::from(ticks_from_ms(100)) + 1);
    assert_eq!(warden.running(), Some((0, Phase::Active)));
    // Before the edge opens the Bash waits for the action.
    warden.bash();
    let mut bashed_at = None;
    for tick in 0..u64::from(ticks_from_ms(500)) {
        if bashed(&warden.tick()) {
            bashed_at = Some(tick);
            break;
        }
    }
    assert!(
        bashed_at.is_some(),
        "the Bash follows the edge, waiving the cooldown"
    );
    assert_eq!(warden.running(), None);
}

#[test]
fn jump_and_attack_is_more_specific_than_the_plain_binding() {
    static BOTH: ActionGraph = ActionGraph {
        authored: &[
            AuthoredAction {
                slot: 0,
                name: "Heavy slash",
                trigger: Trigger {
                    context: InputContext::Direct,
                    ..SLIDING_SLASH.trigger
                },
                effects: &[],
                ..SLIDING_SLASH
            },
            AuthoredAction {
                slot: 1,
                name: "Diving slash",
                trigger: Trigger {
                    context: InputContext::Airborne,
                    ..SLIDING_SLASH.trigger
                },
                effects: &[],
                ..SLIDING_SLASH
            },
        ],
        ..action::BASELINE
    };
    let mut warden = Hero::new(PlayerClass::Warden, &BOTH);
    let reports = warden.inspect().authored;
    assert_eq!(reports[0].status, TriggerStatus::Accepted);
    assert_eq!(
        reports[1].status,
        TriggerStatus::Refused(TriggerRefusal::Context(InputContext::Airborne))
    );
    warden.send(ZoneCommand::Jump);
    warden.ticks(mmorpg_core::golden::t30(3));
    let reports = warden.inspect().authored;
    assert_eq!(reports[0].status, TriggerStatus::Outranked(1));
    assert_eq!(reports[1].status, TriggerStatus::Accepted);
    warden.strike();
    warden.tick();
    assert_eq!(warden.offense().action, Some(ActionId::Authored(1)));
}

#[test]
fn validation_reveals_unsupported_primitives_and_conflicts() {
    let check = |authored: &'static [AuthoredAction], edges: &'static [EdgeDef]| {
        ActionGraph {
            authored,
            edges,
            ..action::BASELINE
        }
        .validate()
    };
    let base: &'static [EdgeDef] = &action::EDGES;
    assert_eq!(check(&[SLIDING_SLASH], base), Ok(()));
    // Movement triggers need a primitive that does not exist.
    static MOVE: [AuthoredAction; 1] = [AuthoredAction {
        trigger: Trigger {
            input: ActionInput::Move,
            ability: None,
            context: InputContext::Direct,
        },
        ..SLIDING_SLASH
    }];
    assert_eq!(
        check(&MOVE, base),
        Err(GraphError::BadAuthored(0, AuthoredProblem::Input))
    );
    // Warden melee is no Ranger input; Firebolt is no Warden ability.
    static RANGER: [AuthoredAction; 1] = [AuthoredAction {
        class: PlayerClass::Ranger,
        ..SLIDING_SLASH
    }];
    assert_eq!(
        check(&RANGER, base),
        Err(GraphError::BadAuthored(0, AuthoredProblem::Input))
    );
    static FOREIGN: [AuthoredAction; 1] = [AuthoredAction {
        trigger: Trigger {
            input: ActionInput::Ability,
            ability: Some(ids::FIREBOLT),
            context: InputContext::Direct,
        },
        ..SLIDING_SLASH
    }];
    assert_eq!(
        check(&FOREIGN, base),
        Err(GraphError::BadAuthored(0, AuthoredProblem::Ability))
    );
    // A confirmed target needs an edge source that confirms one.
    static TARGETED: [AuthoredAction; 1] = [AuthoredAction {
        effects: &[Effect::TargetConfirmed],
        ..SLIDING_SLASH
    }];
    assert_eq!(
        check(&TARGETED, base),
        Err(GraphError::BadAuthored(0, AuthoredProblem::Effect))
    );
    static ZERO_ACTIVE: [AuthoredAction; 1] = [AuthoredAction {
        active: AuthoredPhase::free(0),
        ..SLIDING_SLASH
    }];
    assert_eq!(
        check(&ZERO_ACTIVE, base),
        Err(GraphError::BadAuthored(0, AuthoredProblem::Duration))
    );
    static TWICE: [AuthoredAction; 2] = [
        SLIDING_SLASH,
        AuthoredAction {
            slot: 1,
            name: "Copy",
            ..SLIDING_SLASH
        },
    ];
    assert_eq!(
        check(&TWICE, base),
        Err(GraphError::ConflictingTrigger(0, 1))
    );
    // Edges must leave an existing authored action and end before it does.
    assert_eq!(
        check(&[], &[SLIDE_TO_BASH]),
        Err(GraphError::UnknownAuthored(0))
    );
    static LATE: [EdgeDef; 1] = [EdgeDef {
        window: Window {
            open_after_ms: 300,
            actionable_ms: 300,
            buffer_ms: 0,
        },
        ..SLIDE_TO_BASH
    }];
    assert_eq!(
        check(&[SLIDING_SLASH], &LATE),
        Err(GraphError::OutlastsSource(EdgeId::Authored(0)))
    );
}
