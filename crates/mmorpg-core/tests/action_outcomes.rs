//! Typed outcomes, precision windows and phase-aware interruptions of the
//! action graph (#380, PR 2, Atlas 5.7.3, 5.8.3–5.8.4). The baseline keeps
//! today's stun, guard-break and death behavior and explains it; precision
//! windows, window modifiers, effects and other interruptions run only
//! through a validated test configuration.
mod support;

use std::sync::Arc;

use mmorpg_core::ability::ids;
use mmorpg_core::action::{
    self, ActionGraph, ActionId, ActionInput, BlockReason, Candidate, Condition, Consequence,
    EdgeDef, EdgeId, EdgeStatus, Effect, GraphError, Interrupted, InterruptionReport,
    InterruptionRule, Outcome, Precision, Region, RuleId, WindowModifier,
};
use mmorpg_core::{
    Aura, CanonicalPlayerSnapshot, CreatureBehaviour, CreatureTemplate, EntityRef, ErrorCode,
    PlayerCombatPolicy, StrikeAim, StrikeHeight, StrikeSide, ZoneAreas, ZoneCommand, ZoneContent,
    ZoneEvent, ZoneId, ZoneSimulation,
};
use support::arena::{self, GRAVEYARD, WOLF};

const SOUTH: u16 = 32_768;
const IMPACT: u64 = mmorpg_core::COMBO_IMPACT_TICKS as u64;

/// The combo edge with a 100 ms sweet spot at its opening that shortens the
/// follow-up's recovery to 0.5 s and refunds 5 Rage.
static PRECISE_EDGES: [EdgeDef; 1] = [EdgeDef {
    precision: Some(Precision {
        after_open_ms: 0,
        length_ms: 100,
        benefits: &[
            Effect::Recovery { ms: 500 },
            Effect::ResourceRefund { amount: 5 },
        ],
    }),
    ..action::EDGES[0]
}];

/// A test configuration: the precise edge, its actionable window reduced by
/// 300 ms for Wardens, and the baseline rules and interruptions.
static PRECISE: ActionGraph = ActionGraph {
    edges: &PRECISE_EDGES,
    window_modifiers: &[WindowModifier {
        edge: EdgeId::SlashToShieldBash,
        conditions: &[Condition::Warden],
        actionable_delta_ms: -300,
    }],
    ..action::BASELINE
};

/// A test configuration in which a stun suspends the combo opening.
static SUSPENDING: ActionGraph = ActionGraph {
    interruptions: &[InterruptionRule {
        reaction: ActionId::Stun,
        interrupted: Interrupted::Edge(EdgeId::SlashToShieldBash),
        phase: None,
        consequence: Consequence::Suspend,
    }],
    ..action::BASELINE
};

/// A test configuration in which a stun during the opening's follow-up
/// phase cancels it.
static CANCELLING: ActionGraph = ActionGraph {
    interruptions: &[InterruptionRule {
        reaction: ActionId::Stun,
        interrupted: Interrupted::Edge(EdgeId::SlashToShieldBash),
        phase: Some(action::Phase::FollowUp),
        consequence: Consequence::Cancel,
    }],
    ..action::BASELINE
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
        .unwrap()
        // A hostile second Warden can stun the first.
        .with_player_combat(PlayerCombatPolicy::FreeForAll),
    )
}

struct Warden {
    zone: ZoneSimulation,
    content: Arc<ZoneContent>,
    graph: &'static ActionGraph,
    sequence: u32,
}

impl Warden {
    /// A level-2 Warden (Shield Bash learned) with full Rage, running
    /// `graph`, and a hostile level-2 Warden 15 m behind it.
    fn new(graph: &'static ActionGraph) -> Self {
        let content = content();
        let mut zone = ZoneSimulation::with_content(ZoneId::new(9), Arc::clone(&content)).unwrap();
        zone.add_player(1).unwrap();
        zone.add_player(2).unwrap();
        zone.apply_command(2, 1, ZoneCommand::ChooseClass { class: 0, sex: 0 })
            .unwrap();
        let mut warden = Self {
            zone,
            content,
            graph,
            sequence: 0,
        };
        warden.send(ZoneCommand::ChooseClass { class: 0, sex: 0 });
        warden.send(arena::stand(SOUTH));
        for _ in 0..mmorpg_core::golden::t30(20) {
            warden.tick();
        }
        warden.restore(|players| {
            for player in players.iter_mut() {
                player.combat.level = 2;
                player.combat.health = mmorpg_core::unit::player_max_health(2);
                player.combat.abilities.resource = 100;
            }
            let [x, y, z] = players[0].position;
            players[1].position = [x, y, z + 1_500.0];
        });
        warden
    }

    /// Edits the canonical players and restores the zone, keeping its graph.
    fn restore(&mut self, edit: impl FnOnce(&mut [CanonicalPlayerSnapshot])) {
        let mut state = self.zone.snapshot().unwrap();
        edit(&mut state.players);
        self.zone = ZoneSimulation::from_snapshot(state, Arc::clone(&self.content)).unwrap();
        self.zone.set_action_graph(self.graph).unwrap();
    }

    /// An incoming stun for `ticks` from the other Warden's Shield Bash.
    fn stun(&mut self, ticks: u16) {
        self.restore(|players| {
            players[0].combat.abilities.auras.push(Aura {
                ability: ids::SHIELD_BASH,
                caster: EntityRef::Player(2),
                remaining: ticks,
                amount: 0,
            });
        });
    }

    fn send(&mut self, command: ZoneCommand) {
        self.sequence += 1;
        self.zone.apply_command(1, self.sequence, command).unwrap();
    }

    fn tick(&mut self) -> Vec<ZoneEvent> {
        self.zone.advance_tick().unwrap();
        self.zone.snapshot_for_player(1).unwrap().events
    }

    fn inspect(&self) -> action::ActionInspection {
        self.zone.action_inspection(1).unwrap()
    }

    fn arbitrate(&self) -> action::Arbitration {
        self.zone
            .arbitrate_input(1, ActionInput::ShieldBash)
            .unwrap()
    }

    fn combat(&self) -> mmorpg_core::CanonicalPlayerCombat {
        self.zone.snapshot().unwrap().players[0].combat.clone()
    }

    /// A centred directed slash into the wolf; the tick it resolved.
    fn slash(&mut self) -> u64 {
        self.send(ZoneCommand::DirectedStrike {
            ability: ids::HEROIC_STRIKE.get(),
            facing: SOUTH,
            aim: StrikeAim::new(StrikeSide::Centre, StrikeHeight::Middle),
        });
        self.tick();
        assert!(self.combat().combo.is_some(), "the slash confirms");
        self.inspect().tick
    }

    fn bash(&mut self) {
        self.send(ZoneCommand::UseAbility {
            ability: ids::SHIELD_BASH.get(),
            target: None,
        });
    }

    /// Ticks until the Shield Bash resolves: its tick and whether it was
    /// used (`None`) or refused (the code).
    fn bash_resolution(&mut self) -> (u64, Option<ErrorCode>) {
        for _ in 0..mmorpg_core::golden::t30(90) {
            for event in self.tick() {
                match event {
                    ZoneEvent::AbilityUsed { ability, .. } if ability == ids::SHIELD_BASH => {
                        return (self.inspect().tick, None);
                    }
                    ZoneEvent::Error { code, .. } => return (self.inspect().tick, Some(code)),
                    _ => {}
                }
            }
        }
        panic!("the Bash resolves");
    }

    fn until(&mut self, tick: u64) {
        while self.inspect().tick < tick {
            self.tick();
        }
    }
}

fn report(
    interrupted: Interrupted,
    reaction: ActionId,
    consequence: Consequence,
    authored: bool,
) -> InterruptionReport {
    InterruptionReport {
        interrupted,
        reaction,
        consequence,
        authored,
    }
}

#[test]
fn an_incoming_stun_cancels_the_buffered_guard_and_keeps_the_opening() {
    let mut warden = Warden::new(&action::BASELINE);
    assert!(warden.inspect().interruptions.is_empty());
    let strike = warden.slash();
    warden.send(ZoneCommand::Guard {
        lane: Some(StrikeHeight::Middle),
    });
    warden.bash();
    warden.tick();
    assert_eq!(
        warden.inspect().regions[Region::Defense as usize].action,
        Some(ActionId::Guard)
    );
    warden.stun(mmorpg_core::ticks_from_ms(1_000));

    let inspection = warden.inspect();
    assert_eq!(
        inspection.regions[Region::Reaction as usize].action,
        Some(ActionId::Stun)
    );
    assert_eq!(
        inspection.interruptions,
        [
            report(
                Interrupted::Action(ActionId::DirectedSlash),
                ActionId::Stun,
                Consequence::Preserve,
                false
            ),
            report(
                Interrupted::Action(ActionId::Guard),
                ActionId::Stun,
                Consequence::Suspend,
                true
            ),
            report(
                Interrupted::Edge(EdgeId::SlashToShieldBash),
                ActionId::Stun,
                Consequence::Preserve,
                true
            ),
            report(
                Interrupted::Held(RuleId::SlashRecoveryHoldsGuard),
                ActionId::Stun,
                Consequence::Cancel,
                true
            ),
        ]
    );
    // The preserved opening keeps its ticks and still holds the Bash.
    assert_eq!(inspection.edges[0].status, EdgeStatus::Buffering);
    assert_eq!(inspection.edges[0].open_from, Some(strike + IMPACT));
    assert_eq!(
        warden.arbitrate().outcome,
        Outcome::WaitForEdge(EdgeId::SlashToShieldBash)
    );

    // The cancellation lowers the buffered guard at the end of the next tick.
    warden.tick();
    assert_eq!(warden.combat().abilities.guard.raised, None);
    assert_eq!(
        warden.inspect().regions[Region::Defense as usize].action,
        None
    );
    // The opening resolves the Bash at the inspected tick, and the stun
    // refuses it, as before the graph.
    assert_eq!(
        warden.bash_resolution(),
        (strike + IMPACT, Some(ErrorCode::Stunned))
    );
}

#[test]
fn a_precise_follow_up_gets_its_typed_benefits() {
    // The baseline: the buffered Bash resolves when the edge opens.
    let mut baseline = Warden::new(&action::BASELINE);
    let strike = baseline.slash();
    baseline.bash();
    assert_eq!(baseline.bash_resolution(), (strike + IMPACT, None));
    let plain = baseline.combat().abilities;

    let mut warden = Warden::new(&PRECISE);
    assert_eq!(warden.slash(), strike);
    let edge = warden.inspect().edges[0];
    // 100 ms precise from the opening; 600 − 300 ms actionable.
    assert_eq!(edge.precision, Some((strike + IMPACT, strike + IMPACT + 6)));
    assert_eq!(edge.open_until, Some(strike + IMPACT + 18));
    warden.bash();
    warden.until(strike + IMPACT - 1);
    let arbitration = warden.arbitrate();
    assert_eq!(
        arbitration.outcome,
        Outcome::Proceed(Candidate::Edge(EdgeId::SlashToShieldBash))
    );
    assert!(arbitration.precise);
    assert_eq!(warden.bash_resolution(), (strike + IMPACT, None));
    let precise = warden.combat().abilities;
    // A 0.5 s recovery instead of 1.5 s, and 5 of the 10 Rage back.
    assert_eq!(
        plain.global_cooldown - precise.global_cooldown,
        mmorpg_core::GLOBAL_COOLDOWN_TICKS - mmorpg_core::ticks_from_ms(500)
    );
    assert_eq!(precise.resource, plain.resource + 5);
}

#[test]
fn a_late_follow_up_stays_valid_and_the_reduced_window_expires_early() {
    let mut warden = Warden::new(&PRECISE);
    let strike = warden.slash();
    // After the sweet spot, the follow-up is ordinary.
    warden.until(strike + IMPACT + 9);
    let arbitration = warden.arbitrate();
    assert_eq!(
        arbitration.outcome,
        Outcome::Proceed(Candidate::Edge(EdgeId::SlashToShieldBash))
    );
    assert!(!arbitration.precise);

    // A second slash: past the reduced window the edge has expired, where
    // the baseline window would still be open.
    let mut warden = Warden::new(&PRECISE);
    let strike = warden.slash();
    warden.until(strike + IMPACT + 18);
    assert_eq!(
        warden.inspect().edges[0].status,
        EdgeStatus::Blocked(BlockReason::SourceInactive)
    );
    assert!(mmorpg_core::COMBO_WINDOW_TICKS as u64 > 18);
    assert_eq!(
        warden.arbitrate().outcome,
        Outcome::Proceed(Candidate::Direct)
    );
}

#[test]
fn a_configured_stun_suspends_or_cancels_the_opening() {
    // Suspended: the opening's clock stops for the stun and the Bash waits.
    let mut warden = Warden::new(&SUSPENDING);
    let strike = warden.slash();
    warden.until(strike + IMPACT);
    let stun = mmorpg_core::ticks_from_ms(1_000);
    warden.stun(stun);
    assert_eq!(
        warden.inspect().edges[0].status,
        EdgeStatus::Suspended(ActionId::Stun)
    );
    warden.bash();
    assert_eq!(
        warden.arbitrate().outcome,
        Outcome::WaitForEdge(EdgeId::SlashToShieldBash)
    );
    let (resolved, refusal) = warden.bash_resolution();
    assert_eq!(refusal, None);
    // Long after the baseline window would have closed.
    assert!(resolved > strike + IMPACT + mmorpg_core::COMBO_WINDOW_TICKS as u64);
    // The stun, applied after tick `strike + IMPACT`, holds the next `stun`
    // ticks; the Bash resolves on the first tick after it.
    assert_eq!(resolved, strike + IMPACT + u64::from(stun) + 1);

    // Cancelled in the follow-up phase: the opening ends and the Bash is
    // a direct use.
    let mut warden = Warden::new(&CANCELLING);
    let strike = warden.slash();
    warden.stun(mmorpg_core::ticks_from_ms(100));
    // Still in the slash's recovery: the rule's phase does not match.
    assert_eq!(warden.inspect().edges[0].status, EdgeStatus::Buffering);
    warden.until(strike + IMPACT);
    warden.stun(mmorpg_core::ticks_from_ms(100));
    assert_eq!(
        warden.inspect().edges[0].status,
        EdgeStatus::Blocked(BlockReason::Cancelled(ActionId::Stun))
    );
    warden.tick();
    assert_eq!(warden.combat().combo, None);
    assert_eq!(
        warden.arbitrate().outcome,
        Outcome::Proceed(Candidate::Direct)
    );
}

#[test]
fn invalid_configurations_are_refused() {
    action::validate().unwrap();
    PRECISE.validate().unwrap();
    static OUTSIDE: [EdgeDef; 1] = [EdgeDef {
        precision: Some(Precision {
            after_open_ms: 500,
            length_ms: 200,
            benefits: &[],
        }),
        ..action::EDGES[0]
    }];
    static OUTSIDE_GRAPH: ActionGraph = ActionGraph {
        edges: &OUTSIDE,
        ..action::BASELINE
    };
    let mut zone = ZoneSimulation::new(ZoneId::new(1));
    assert_eq!(
        zone.set_action_graph(&OUTSIDE_GRAPH),
        Err(GraphError::PrecisionOutsideWindow(
            EdgeId::SlashToShieldBash
        ))
    );
    assert_eq!(zone.action_graph(), &action::BASELINE);
    static CONFLICT: ActionGraph = ActionGraph {
        interruptions: &[
            InterruptionRule {
                reaction: ActionId::Stun,
                interrupted: Interrupted::Action(ActionId::Guard),
                phase: None,
                consequence: Consequence::Suspend,
            },
            InterruptionRule {
                reaction: ActionId::Stun,
                interrupted: Interrupted::Action(ActionId::Guard),
                phase: None,
                consequence: Consequence::Cancel,
            },
        ],
        ..action::BASELINE
    };
    assert_eq!(
        CONFLICT.validate(),
        Err(GraphError::ConflictingInterruption(Interrupted::Action(
            ActionId::Guard
        )))
    );
    static SUSPEND_HELD: ActionGraph = ActionGraph {
        interruptions: &[InterruptionRule {
            reaction: ActionId::Death,
            interrupted: Interrupted::Held(RuleId::LandingHoldsMelee),
            phase: None,
            consequence: Consequence::Suspend,
        }],
        ..action::BASELINE
    };
    assert_eq!(
        SUSPEND_HELD.validate(),
        Err(GraphError::UnsupportedInterruption(Interrupted::Held(
            RuleId::LandingHoldsMelee
        )))
    );
    static SHRUNK: ActionGraph = ActionGraph {
        window_modifiers: &[WindowModifier {
            edge: EdgeId::SlashToShieldBash,
            conditions: &[],
            actionable_delta_ms: -700,
        }],
        ..action::BASELINE
    };
    assert_eq!(
        SHRUNK.validate(),
        Err(GraphError::NegativeWindow(EdgeId::SlashToShieldBash))
    );
    static REFUND_NOTHING: [EdgeDef; 1] = [EdgeDef {
        effects: &[
            Effect::TargetConfirmed,
            Effect::ResourceRefund { amount: 0 },
        ],
        ..action::EDGES[0]
    }];
    assert_eq!(
        ActionGraph {
            edges: &REFUND_NOTHING,
            ..action::BASELINE
        }
        .validate(),
        Err(GraphError::BadEffect(EdgeId::SlashToShieldBash))
    );
}
