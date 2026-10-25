//! Traversal, movement momentum and combat transitions on the one action
//! graph (#413, Atlas 5.9.1–5.9.3): the movement region runs for every
//! class, action phases author bounded movement that Rapier's controller
//! resolves, and edges out of a landing let a run → jump → aerial attack →
//! landing → follow-up path compose from ordinary actions. Experimental
//! graphs author all of it; the production graph keeps today's movement.
mod support;

use std::sync::Arc;

use mmorpg_core::ability::ids;
use mmorpg_core::action::{
    self, ActionGraph, ActionId, ActionInput, Anchor, Candidate, Condition, EdgeDef, EdgeId,
    EdgeStatus, Effect, GraphError, Key, Movement, MovementRule, Origin, Outcome, Override, Phase,
    Region, Scope, Setting, Verdict, Window,
};
use mmorpg_core::{
    CreatureBehaviour, CreatureId, CreatureTemplate, EntityRef, PlayerClass, StrikeAim,
    StrikeHeight, StrikeSide, ZoneAreas, ZoneCommand, ZoneContent, ZoneEvent, ZoneId,
    ZoneSimulation,
};
use support::arena::{self, GRAVEYARD, WOLF};

const NORTH: u16 = 0;
const SOUTH: u16 = 32_768;

/// No air control: a jump keeps the velocity it took off with.
const BALLISTIC: Movement = Movement {
    steering_percent: 0,
    momentum_percent: 100,
    max_acceleration_cms2: None,
};

/// Out of the landing after an airborne melee hit into Shield Bash, 100 ms
/// after touchdown for 100 ms, waiving the global cooldown. It outranks the
/// slash's own combo edge.
const LANDING_BASH: EdgeDef = EdgeDef {
    id: EdgeId::LandingToShieldBash,
    from: ActionId::Landing,
    input: ActionInput::ShieldBash,
    anchor: Anchor::Landing,
    window: Window {
        open_after_ms: 100,
        actionable_ms: 100,
        buffer_ms: 100,
    },
    conditions: &[Condition::Warden, Condition::AttackedAirborne],
    effects: &[Effect::IgnoreGlobalCooldown],
    precision: None,
    priority: 2,
};

static EDGES: [EdgeDef; 2] = [action::EDGES[0], LANDING_BASH];

static AIRBORNE_BALLISTIC: [MovementRule; 1] = [MovementRule {
    action: ActionId::Airborne,
    phase: Phase::Active,
    conditions: &[],
    movement: BALLISTIC,
}];

/// Ballistic jumps and the landing Bash.
static TRAVERSAL: ActionGraph = ActionGraph {
    edges: &EDGES,
    movement: &AIRBORNE_BALLISTIC,
    ..action::BASELINE
};

/// The landing Bash launches the Warden 3 m/s up: a hop out of the landing.
static HOP: ActionGraph = ActionGraph {
    edges: &[
        action::EDGES[0],
        EdgeDef {
            effects: &[
                Effect::IgnoreGlobalCooldown,
                Effect::Impulse {
                    forward_cms: 0,
                    up_cms: 300,
                },
            ],
            ..LANDING_BASH
        },
    ],
    ..action::BASELINE
};

/// Rangers keep full air control; every other class jumps ballistically.
static RANGER_STEERS: ActionGraph = ActionGraph {
    overrides: &[Override {
        scope: Scope::Class(PlayerClass::Ranger),
        setting: Setting::Movement {
            action: ActionId::Airborne,
            phase: Phase::Active,
            movement: Movement::FREE,
        },
    }],
    ..TRAVERSAL
};

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

struct Hero {
    zone: ZoneSimulation,
    sequence: u32,
}

impl Hero {
    /// A level-2 hero of `class` (a Warden has Shield Bash) with a full
    /// resource, running `graph`.
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

    fn movement(&self) -> action::RegionReport {
        self.inspect().regions[Region::Movement as usize]
    }

    fn position(&self) -> [f32; 3] {
        self.zone.snapshot().unwrap().players[0].position
    }

    /// Runs north, jumps, lets go of the movement keys at takeoff and
    /// returns how far it travelled north until it touched down.
    fn running_jump(&mut self) -> f32 {
        self.send(arena::walk(NORTH));
        self.ticks(mmorpg_core::golden::t30(10));
        self.send(ZoneCommand::Jump);
        self.tick();
        self.send(arena::stand(NORTH));
        let from = self.position();
        self.tick();
        assert_eq!(self.movement().action, Some(ActionId::Airborne));
        while self.movement().action == Some(ActionId::Airborne) {
            self.tick();
        }
        assert_eq!(self.movement().action, Some(ActionId::Landing));
        self.position()[2] - from[2]
    }

    /// An airborne directed slash at the wolf; whether it hit.
    fn aerial_slash(&mut self) -> bool {
        self.send(ZoneCommand::Jump);
        self.ticks(mmorpg_core::golden::t30(3));
        assert_eq!(self.movement().action, Some(ActionId::Airborne));
        self.send(ZoneCommand::DirectedStrike {
            ability: ids::HEROIC_STRIKE.get(),
            facing: SOUTH,
            aim: StrikeAim::new(StrikeSide::Centre, StrikeHeight::Low),
        });
        let events = self.tick();
        events
            .iter()
            .any(|event| matches!(event, ZoneEvent::DamageDealt { .. }))
    }

    /// Ticks until touchdown; the landing's inspected anchor tick.
    fn land(&mut self) -> u64 {
        while self.movement().action != Some(ActionId::Landing) {
            self.tick();
        }
        self.inspect().tick + 1
    }

    fn bash(&mut self) {
        self.send(ZoneCommand::UseAbility {
            ability: ids::SHIELD_BASH.get(),
            target: Some(EntityRef::Creature(CreatureId::new(1))),
        });
    }

    /// Ticks until the Shield Bash resolves (used or refused); its tick and
    /// whether it was used.
    fn bash_resolution(&mut self) -> (u64, bool) {
        for _ in 0..mmorpg_core::golden::t30(60) {
            let events = self.tick();
            for event in events {
                match event {
                    ZoneEvent::AbilityUsed { ability, .. } if ability == ids::SHIELD_BASH => {
                        return (self.inspect().tick, true);
                    }
                    ZoneEvent::Error { .. } => return (self.inspect().tick, false),
                    _ => {}
                }
            }
        }
        panic!("the Bash resolves");
    }
}

#[test]
fn the_baseline_moves_freely_and_every_class_runs_the_movement_region() {
    let mut ranger = Hero::new(PlayerClass::Ranger, &action::BASELINE);
    let travelled = ranger.running_jump();
    // Letting go stops a baseline jump at once: it travels at most a tick.
    assert!(travelled.abs() < 15.0, "{travelled}");
    let inspection = ranger.inspect();
    assert_eq!(inspection.movement.movement, Movement::FREE);
    assert!(inspection.movement.sources.is_empty());
    // A Ranger's landing recovers but holds nothing (the hold is Warden melee).
    assert_eq!(
        inspection.regions[Region::Movement as usize].phase,
        Some(Phase::Recovery)
    );
    assert!(inspection.rules.is_empty());
}

#[test]
fn an_authored_ballistic_jump_keeps_its_momentum_through_physics() {
    let mut warden = Hero::new(PlayerClass::Warden, &TRAVERSAL);
    let mut airborne = None;
    warden.send(arena::walk(NORTH));
    warden.ticks(mmorpg_core::golden::t30(10));
    warden.send(ZoneCommand::Jump);
    warden.tick();
    warden.send(arena::stand(NORTH));
    warden.tick();
    let inspection = warden.inspect();
    if inspection.regions[Region::Movement as usize].action == Some(ActionId::Airborne) {
        airborne = Some(inspection.movement.clone());
    }
    let report = airborne.expect("airborne after the jump");
    assert_eq!(report.movement, BALLISTIC);
    assert_eq!(report.sources.len(), 1);
    assert_eq!(report.sources[0].action, ActionId::Airborne);
    assert_eq!(report.sources[0].origin, Origin::Baseline);

    let mut warden = Hero::new(PlayerClass::Warden, &TRAVERSAL);
    let travelled = warden.running_jump();
    // About 6.3 m/s for the whole jump (≈ 1 s at 4.8 m/s up, 9 m/s² down).
    assert!(travelled > 500.0, "{travelled}");
    // On the ground the Warden moves freely again and stops.
    let landed = warden.position();
    warden.ticks(mmorpg_core::golden::t30(10));
    let after = warden.position();
    assert!(
        (after[2] - landed[2]).abs() < 15.0,
        "{landed:?} → {after:?}"
    );
}

#[test]
fn class_overrides_tune_one_class_s_movement_with_provenance() {
    let mut ranger = Hero::new(PlayerClass::Ranger, &RANGER_STEERS);
    assert!(ranger.running_jump().abs() < 15.0);
    assert_eq!(
        ranger.zone.action_graph().resolve(
            Key::Movement(ActionId::Airborne, Phase::Active),
            action::Context {
                class: Some(PlayerClass::Ranger),
                ..action::Context::default()
            }
        ),
        Some(action::Effective {
            setting: Setting::Movement {
                action: ActionId::Airborne,
                phase: Phase::Active,
                movement: Movement::FREE,
            },
            origin: Origin::Override(Scope::Class(PlayerClass::Ranger)),
        })
    );
    let mut warden = Hero::new(PlayerClass::Warden, &RANGER_STEERS);
    assert!(warden.running_jump() > 500.0);
}

#[test]
fn an_aerial_slash_lands_into_an_early_bash_through_the_landing_edge() {
    let mut warden = Hero::new(PlayerClass::Warden, &TRAVERSAL);
    assert!(warden.aerial_slash(), "the aerial slash hits the wolf");
    let anchor = warden.land();
    let inspection = warden.inspect();
    let landing = inspection
        .edges
        .iter()
        .find(|edge| edge.edge == EdgeId::LandingToShieldBash)
        .copied()
        .unwrap();
    assert_eq!(landing.anchor_tick, Some(anchor));
    let opens = anchor + u64::from(mmorpg_core::ticks_from_ms(100));
    assert_eq!(landing.open_from, Some(opens));
    // Before the edge opens, the landing still holds a Bash.
    let arbitration = warden
        .zone
        .arbitrate_input(1, ActionInput::ShieldBash)
        .unwrap();
    assert_eq!(
        arbitration.outcome,
        Outcome::WaitForRule(action::RuleId::LandingHoldsMelee)
    );
    warden.bash();
    let mut checked = false;
    while warden.inspect().tick + 1 < opens {
        warden.tick();
        checked = true;
    }
    assert!(checked);
    let arbitration = warden
        .zone
        .arbitrate_input(1, ActionInput::ShieldBash)
        .unwrap();
    assert_eq!(
        arbitration.outcome,
        Outcome::Proceed(Candidate::Edge(EdgeId::LandingToShieldBash))
    );
    assert!(
        arbitration
            .candidates
            .contains(&(Candidate::Edge(EdgeId::LandingToShieldBash), Verdict::Won))
    );
    // The Bash resolves when the edge opens, within the slash's global
    // cooldown and long before the landing recovery would release it, and
    // it ends the landing.
    assert_eq!(warden.bash_resolution(), (opens, true));
    assert_eq!(warden.movement().action, None);

    // The baseline holds the same Bash for the whole lengthened landing.
    let mut baseline = Hero::new(PlayerClass::Warden, &action::BASELINE);
    assert!(baseline.aerial_slash());
    let anchor = baseline.land();
    baseline.bash();
    let (resolved, _) = baseline.bash_resolution();
    assert_eq!(
        resolved,
        anchor + u64::from(mmorpg_core::OVERHEAD_LANDING_TICKS)
    );
}

#[test]
fn a_landing_edge_refuses_without_its_airborne_attack() {
    let mut warden = Hero::new(PlayerClass::Warden, &TRAVERSAL);
    warden.send(ZoneCommand::Jump);
    warden.tick();
    warden.land();
    let edge = warden
        .inspect()
        .edges
        .iter()
        .find(|edge| edge.edge == EdgeId::LandingToShieldBash)
        .copied()
        .unwrap();
    assert_eq!(
        edge.status,
        EdgeStatus::Blocked(action::BlockReason::Condition(Condition::AttackedAirborne))
    );
}

#[test]
fn an_impulse_follow_up_hops_out_of_the_landing() {
    let mut warden = Hero::new(PlayerClass::Warden, &HOP);
    assert!(warden.aerial_slash());
    warden.land();
    let ground = warden.position()[1];
    warden.bash();
    let (_, used) = warden.bash_resolution();
    assert!(used);
    warden.ticks(mmorpg_core::golden::t30(3));
    assert_eq!(warden.movement().action, Some(ActionId::Airborne));
    assert!(warden.position()[1] > ground + 10.0);
}

#[test]
fn traversal_definitions_fail_closed() {
    let invalid = |graph: ActionGraph| {
        let graph: &'static ActionGraph = Box::leak(Box::new(graph));
        graph.validate().unwrap_err()
    };
    // The landing is over before the window closes.
    static LATE: [EdgeDef; 2] = [
        action::EDGES[0],
        EdgeDef {
            window: Window {
                open_after_ms: 300,
                actionable_ms: 300,
                buffer_ms: 0,
            },
            ..LANDING_BASH
        },
    ];
    assert_eq!(
        invalid(ActionGraph {
            edges: &LATE,
            ..TRAVERSAL
        }),
        GraphError::OutlastsSource(EdgeId::LandingToShieldBash)
    );
    // A landing edge measured from an impact has no runtime.
    static IMPACT: [EdgeDef; 2] = [
        action::EDGES[0],
        EdgeDef {
            anchor: Anchor::Impact,
            ..LANDING_BASH
        },
    ];
    assert_eq!(
        invalid(ActionGraph {
            edges: &IMPACT,
            ..TRAVERSAL
        }),
        GraphError::UnsupportedEdge(EdgeId::LandingToShieldBash)
    );
    // A landing confirms no target; impulses are bounded.
    static KEEPS: [EdgeDef; 2] = [
        action::EDGES[0],
        EdgeDef {
            effects: &[Effect::KeepOpening],
            ..LANDING_BASH
        },
    ];
    static LAUNCH: [EdgeDef; 2] = [
        action::EDGES[0],
        EdgeDef {
            effects: &[Effect::Impulse {
                forward_cms: 2_000,
                up_cms: 0,
            }],
            ..LANDING_BASH
        },
    ];
    for edges in [&KEEPS, &LAUNCH] {
        assert_eq!(
            invalid(ActionGraph { edges, ..TRAVERSAL }),
            GraphError::BadEffect(EdgeId::LandingToShieldBash)
        );
    }
    // Movement covers a phase its action runs in, never a reaction.
    static STUN: [MovementRule; 1] = [MovementRule {
        action: ActionId::Stun,
        phase: Phase::Active,
        conditions: &[],
        movement: BALLISTIC,
    }];
    static RECOVERING_AIR: [MovementRule; 1] = [MovementRule {
        phase: Phase::Recovery,
        ..AIRBORNE_BALLISTIC[0]
    }];
    static OVERSTEER: [MovementRule; 1] = [MovementRule {
        movement: Movement {
            steering_percent: 101,
            ..BALLISTIC
        },
        ..AIRBORNE_BALLISTIC[0]
    }];
    assert_eq!(
        invalid(ActionGraph {
            movement: &STUN,
            ..TRAVERSAL
        }),
        GraphError::BadMovement(ActionId::Stun, Phase::Active)
    );
    for movement in [&RECOVERING_AIR, &OVERSTEER] {
        assert!(matches!(
            invalid(ActionGraph {
                movement,
                ..TRAVERSAL
            }),
            GraphError::BadMovement(ActionId::Airborne, _)
        ));
    }
    static TWICE: [MovementRule; 2] = [AIRBORNE_BALLISTIC[0], AIRBORNE_BALLISTIC[0]];
    assert_eq!(
        invalid(ActionGraph {
            movement: &TWICE,
            ..TRAVERSAL
        }),
        GraphError::DuplicateMovement(ActionId::Airborne, Phase::Active)
    );
    // An override names authored movement.
    assert_eq!(
        invalid(ActionGraph {
            movement: &[],
            ..RANGER_STEERS
        }),
        GraphError::UnknownMovement(ActionId::Airborne, Phase::Active)
    );
}

#[test]
fn movement_mixes_intent_and_momentum_within_its_acceleration_limit() {
    let run = mmorpg_core::RUN_SPEED_SUBUNITS_PER_TICK;
    assert_eq!(Movement::FREE.velocity([run, 0], [0, -run]), [run, 0]);
    assert_eq!(BALLISTIC.velocity([0, 0], [run, 7]), [run, 7]);
    let half = Movement {
        steering_percent: 50,
        momentum_percent: 100,
        max_acceleration_cms2: None,
    };
    assert_eq!(half.velocity([run, 0], [0, 0]), [run / 2, 0]);
    // 600 cm/s² is 600 · 256 / 3600 ≈ 43 subunits per tick per tick.
    let gradual = Movement {
        max_acceleration_cms2: Some(600),
        ..Movement::FREE
    };
    assert_eq!(gradual.velocity([run, 0], [0, 0]), [43, 0]);
    assert_eq!(
        half.combine(gradual),
        Movement {
            steering_percent: 50,
            momentum_percent: 100,
            max_acceleration_cms2: Some(600),
        }
    );
}
