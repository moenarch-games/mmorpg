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
    AuthoredProblem, BlockReason, Direction, EdgeDef, EdgeId, Effect, Gesture, GraphError,
    InputContext, Movement, Origin, Outcome, Partner, Phase, Region, TimedFollowUp, Trigger,
    TriggerRefusal, TriggerStatus, Window,
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
    trigger: Trigger::press(
        ActionInput::WardenMelee,
        Some(ids::HEROIC_STRIKE),
        InputContext::Moving(Direction::Forward),
    ),
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

    /// The Bash key goes up, or `input-bindings` retires its hold.
    fn release_bash(&mut self, retired: bool) {
        self.send(ZoneCommand::ReleaseAbility {
            ability: ids::SHIELD_BASH.get(),
            retired,
        });
    }

    /// A Bash sent after observing tick `observed` (#415).
    fn observed_bash(&mut self, observed: u64) {
        self.sequence += 1;
        self.zone
            .apply_observed_command(
                1,
                self.sequence,
                ZoneCommand::UseAbility {
                    ability: ids::SHIELD_BASH.get(),
                    target: Some(EntityRef::Creature(CreatureId::new(1))),
                },
                observed,
            )
            .unwrap();
    }

    /// Ticks until a Bash resolves, at most `limit`: the tick it did.
    fn until_bashed(&mut self, limit: u64) -> Option<u64> {
        (1..=limit).find(|_| bashed(&self.tick()))
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
        trigger: Trigger::press(ActionInput::Move, None, InputContext::Direct),
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
        trigger: Trigger::press(
            ActionInput::Ability,
            Some(ids::FIREBOLT),
            InputContext::Direct,
        ),
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

/// Hold Shield Bash to charge it: released after at least 200 ms (or held
/// the whole 600 ms startup) it bashes with a launch; a quicker tap is the
/// plain Bash.
static CHARGED: ActionGraph = ActionGraph {
    authored: &[AuthoredAction {
        slot: 0,
        name: "Charged bash",
        class: PlayerClass::Warden,
        trigger: Trigger {
            gesture: Gesture::Charge { min_ms: 200 },
            ..Trigger::press(
                ActionInput::ShieldBash,
                Some(ids::SHIELD_BASH),
                InputContext::Direct,
            )
        },
        conditions: &[],
        startup: AuthoredPhase::free(600),
        active: AuthoredPhase::free(100),
        recovery: AuthoredPhase::free(100),
        effects: &[Effect::Impulse {
            forward_cms: 300,
            up_cms: 0,
        }],
    }],
    ..action::BASELINE
};

#[test]
fn a_charge_releases_after_its_minimum_and_a_tap_is_the_plain_use() {
    // Released 300 ms after the press: charged.
    let mut warden = Hero::new(PlayerClass::Warden, &CHARGED);
    warden.bash();
    assert!(!bashed(&warden.tick()), "the press starts charging");
    assert_eq!(warden.running(), Some((0, Phase::Startup)));
    assert_eq!(warden.until_bashed(u64::from(ticks_from_ms(300))), None);
    warden.release_bash(false);
    assert!(bashed(&warden.tick()), "the release bashes at once");
    assert_eq!(warden.running(), Some((0, Phase::Active)));

    // Released 100 ms after the press: a tap, the ordinary Bash.
    let mut warden = Hero::new(PlayerClass::Warden, &CHARGED);
    warden.bash();
    warden.ticks(u64::from(ticks_from_ms(100)));
    warden.release_bash(false);
    assert!(bashed(&warden.tick()));
    assert_eq!(warden.running(), None);

    // Held throughout: the full charge fires as the startup ends.
    let mut warden = Hero::new(PlayerClass::Warden, &CHARGED);
    warden.bash();
    let full = u64::from(ticks_from_ms(600));
    assert_eq!(warden.until_bashed(full + 1), Some(full + 1));
    assert_eq!(warden.running(), Some((0, Phase::Active)));

    // A retired hold (focus loss, a menu) never acts.
    let mut warden = Hero::new(PlayerClass::Warden, &CHARGED);
    warden.bash();
    warden.ticks(u64::from(ticks_from_ms(300)));
    warden.release_bash(true);
    assert_eq!(warden.until_bashed(full), None);
    assert_eq!(warden.running(), None);
}

#[test]
fn the_production_graph_ignores_releases() {
    assert!(!action::BASELINE.tracks_holds());
    let mut warden = Hero::new(PlayerClass::Warden, &action::BASELINE);
    warden.bash();
    warden.release_bash(false);
    assert!(bashed(&warden.tick()), "the press is the Bash, as before");
}

/// Shield Bash while the guard is held, or while Heroic Strike is held.
static CHORDS: ActionGraph = ActionGraph {
    authored: &[
        AuthoredAction {
            slot: 0,
            name: "Guarded bash",
            class: PlayerClass::Warden,
            trigger: Trigger {
                gesture: Gesture::Chord(Partner::Guard),
                ..Trigger::press(ActionInput::ShieldBash, None, InputContext::Direct)
            },
            conditions: &[],
            startup: AuthoredPhase::free(100),
            active: AuthoredPhase::free(100),
            recovery: AuthoredPhase::free(100),
            effects: &[],
        },
        AuthoredAction {
            slot: 1,
            name: "Double strike",
            class: PlayerClass::Warden,
            trigger: Trigger {
                gesture: Gesture::Chord(Partner::Input(ActionInput::WardenMelee)),
                ..Trigger::press(ActionInput::ShieldBash, None, InputContext::Direct)
            },
            conditions: &[],
            startup: AuthoredPhase::free(100),
            active: AuthoredPhase::free(100),
            recovery: AuthoredPhase::free(100),
            effects: &[],
        },
    ],
    ..action::BASELINE
};

#[test]
fn a_chord_needs_its_partner_held() {
    let mut warden = Hero::new(PlayerClass::Warden, &CHORDS);
    let reports = warden.inspect().authored;
    assert_eq!(
        reports[0].status,
        TriggerStatus::Refused(TriggerRefusal::Partner(Partner::Guard))
    );
    assert_eq!(
        reports[1].status,
        TriggerStatus::Refused(TriggerRefusal::Partner(Partner::Input(
            ActionInput::WardenMelee
        )))
    );
    // Heroic Strike pressed and held is the second chord's partner.
    warden.send(ZoneCommand::UseAbility {
        ability: ids::HEROIC_STRIKE.get(),
        target: Some(EntityRef::Creature(CreatureId::new(1))),
    });
    warden.tick();
    assert!(!matches!(
        warden.inspect().authored[1].status,
        TriggerStatus::Refused(TriggerRefusal::Partner(_))
    ));
    warden.send(ZoneCommand::ReleaseAbility {
        ability: ids::HEROIC_STRIKE.get(),
        retired: false,
    });
    warden.tick();
    assert_eq!(
        warden.inspect().authored[1].status,
        TriggerStatus::Refused(TriggerRefusal::Partner(Partner::Input(
            ActionInput::WardenMelee
        )))
    );

    // With the guard raised the Bash starts the guarded bash.
    let mut warden = Hero::new(PlayerClass::Warden, &CHORDS);
    warden.send(ZoneCommand::Guard {
        lane: Some(StrikeHeight::High),
    });
    warden.tick();
    assert_eq!(warden.inspect().authored[0].status, TriggerStatus::Accepted);
    warden.bash();
    warden.tick();
    assert_eq!(warden.running(), Some((0, Phase::Startup)));
}

/// The sliding slash, then Shield Bash as its timed follow-up 100–300 ms
/// after the slash's use, buffered up to `buffer_ms` before that.
const fn finisher(buffer_ms: u32) -> AuthoredAction {
    AuthoredAction {
        slot: 1,
        name: "Bash finisher",
        class: PlayerClass::Warden,
        trigger: Trigger {
            follows: Some(TimedFollowUp {
                source: 0,
                window: Window {
                    open_after_ms: 100,
                    actionable_ms: 200,
                    buffer_ms,
                },
            }),
            ..Trigger::press(ActionInput::ShieldBash, None, InputContext::Direct)
        },
        conditions: &[],
        startup: AuthoredPhase::free(0),
        active: AuthoredPhase::free(100),
        recovery: AuthoredPhase::free(100),
        effects: &[Effect::IgnoreGlobalCooldown],
    }
}

static FOLLOWED: ActionGraph = ActionGraph {
    authored: &[SLIDING_SLASH, finisher(200)],
    ..action::BASELINE
};

static TIGHT: ActionGraph = ActionGraph {
    authored: &[SLIDING_SLASH, finisher(50)],
    ..action::BASELINE
};

#[test]
fn a_follow_up_pressed_in_the_source_startup_waits_for_its_window() {
    let mut warden = Hero::new(PlayerClass::Warden, &FOLLOWED);
    assert_eq!(
        warden.inspect().authored[1].status,
        TriggerStatus::Refused(TriggerRefusal::FollowUp(BlockReason::SourceInactive))
    );
    warden.send(arena::walk(SOUTH));
    warden.tick();
    warden.strike();
    warden.tick();
    assert_eq!(warden.running(), Some((0, Phase::Startup)));
    // 200 ms of buffer reach back into the 100 ms startup: an edge's buffer
    // could not precede the use.
    assert_eq!(
        warden.inspect().authored[1].status,
        TriggerStatus::Buffering
    );
    warden.bash();
    let opens = u64::from(ticks_from_ms(100) + ticks_from_ms(100));
    let bashed_at = warden.until_bashed(opens + 2);
    assert_eq!(bashed_at, Some(opens), "the Bash waits for the window");
    assert_eq!(warden.running(), Some((1, Phase::Active)));
}

#[test]
fn a_follow_up_pressed_before_its_buffer_is_lost() {
    let mut warden = Hero::new(PlayerClass::Warden, &TIGHT);
    warden.send(arena::walk(SOUTH));
    warden.tick();
    warden.strike();
    warden.tick();
    warden.bash();
    warden.tick();
    assert_eq!(
        warden.inspect().authored[1].status,
        TriggerStatus::Refused(TriggerRefusal::Missed)
    );
    // The early Bash waits for the slash like any input and never starts
    // the finisher.
    for _ in 0..ticks_from_ms(600) {
        warden.tick();
        assert_ne!(warden.running().map(|(slot, _)| slot), Some(1));
    }
}

#[test]
fn a_late_follow_up_keeps_the_grace_but_nothing_is_backdated() {
    let closes = u64::from(ticks_from_ms(100) + ticks_from_ms(100) + ticks_from_ms(200));
    let follow = |late: u64| {
        let mut warden = Hero::new(PlayerClass::Warden, &FOLLOWED);
        warden.send(arena::walk(SOUTH));
        warden.tick();
        warden.strike();
        // Two ticks after the window closed.
        warden.ticks(closes + 2);
        // The Bash lands where the Warden faces (#517): it turns back to the wolf it passed.
        let face = arena::face(&warden.zone, 1, EntityRef::Creature(CreatureId::new(1)));
        warden.send(face);
        let now = warden.zone.current_tick();
        warden.observed_bash(now - late);
        warden.tick();
        warden.running().map(|(slot, _)| slot)
    };
    assert_eq!(follow(3), Some(1), "the lateness made up for the window");
    assert_eq!(follow(0), Some(0), "an on-time Bash after it waits");
}

#[test]
fn gestures_and_follow_ups_are_validated() {
    let check = |authored: &'static [AuthoredAction]| {
        ActionGraph {
            authored,
            ..action::BASELINE
        }
        .validate()
    };
    assert_eq!(check(CHARGED.authored), Ok(()));
    assert_eq!(check(CHORDS.authored), Ok(()));
    assert_eq!(check(FOLLOWED.authored), Ok(()));
    // A charge's minimum fits its startup.
    static LONG: [AuthoredAction; 1] = [AuthoredAction {
        trigger: Trigger {
            gesture: Gesture::Charge { min_ms: 700 },
            ..CHARGED.authored[0].trigger
        },
        ..CHARGED.authored[0]
    }];
    assert_eq!(
        check(&LONG),
        Err(GraphError::BadAuthored(0, AuthoredProblem::Charge))
    );
    // Only Wardens hold a guard.
    static RANGER: [AuthoredAction; 1] = [AuthoredAction {
        class: PlayerClass::Ranger,
        trigger: Trigger {
            gesture: Gesture::Chord(Partner::Guard),
            ..Trigger::press(ActionInput::Ability, None, InputContext::Direct)
        },
        ..CHARGED.authored[0]
    }];
    assert_eq!(
        check(&RANGER),
        Err(GraphError::BadAuthored(0, AuthoredProblem::Chord))
    );
    // A charge and a press of one input conflict; a chord does not.
    static BOTH: [AuthoredAction; 3] = [
        CHARGED.authored[0],
        AuthoredAction {
            slot: 1,
            trigger: Trigger::press(
                ActionInput::ShieldBash,
                Some(ids::SHIELD_BASH),
                InputContext::Direct,
            ),
            ..CHARGED.authored[0]
        },
        AuthoredAction {
            slot: 2,
            ..CHORDS.authored[0]
        },
    ];
    assert_eq!(check(&BOTH), Err(GraphError::ConflictingTrigger(0, 1)));
    static PAIR: [AuthoredAction; 2] = [BOTH[0], BOTH[2]];
    assert_eq!(check(&PAIR), Ok(()));
    // A follow-up buffers back at most to its source's trigger, closes
    // before the source ends and follows an earlier action.
    static DEEP: [AuthoredAction; 2] = [SLIDING_SLASH, finisher(250)];
    assert_eq!(
        check(&DEEP),
        Err(GraphError::BadAuthored(1, AuthoredProblem::FollowUp))
    );
    static FIRST: [AuthoredAction; 2] = [finisher(200), SLIDING_SLASH];
    assert_eq!(
        check(&FIRST),
        Err(GraphError::BadAuthored(1, AuthoredProblem::FollowUp))
    );
}
