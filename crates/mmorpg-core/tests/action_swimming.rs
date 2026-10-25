//! Swimming as a movement-region action (#413, Atlas 5.1.41–5.1.43): a
//! living player in deep water runs [`ActionId::Swim`], and compatibility
//! rules can refuse an attack in water or hold it until the swimmer is
//! ashore. Experimental graphs author those rules; the production graph
//! keeps allowing attacks while swimming.
mod support;

use std::sync::Arc;

use mmorpg_core::ability::ids;
use mmorpg_core::action::{
    self, ActionGraph, ActionId, ActionInput, Candidate, CompatibilityRule, GraphError, Movement,
    MovementRule, Outcome, Phase, Region, RuleEffect, RuleId, Verdict,
};
use mmorpg_core::{
    CreatureBehaviour, CreatureId, EntityRef, ErrorCode, PlayerClass, WaterVolume, ZoneAreas,
    ZoneCommand, ZoneContent, ZoneEvent, ZoneId, ZoneSimulation,
};
use support::arena::{self, GRAVEYARD, WOLF};

const SOUTH: u16 = 32_768;

/// Melee and Shield Bash are refused in water; other abilities wait ashore.
const WATER_RULES: [CompatibilityRule; 3] = [
    CompatibilityRule {
        id: RuleId::SwimmingRefuses,
        during: ActionId::Swim,
        input: ActionInput::WardenMelee,
        conditions: &[],
        effect: RuleEffect::Refuse,
    },
    CompatibilityRule {
        id: RuleId::SwimmingRefuses,
        during: ActionId::Swim,
        input: ActionInput::ShieldBash,
        conditions: &[],
        effect: RuleEffect::Refuse,
    },
    CompatibilityRule {
        id: RuleId::SwimmingHolds,
        during: ActionId::Swim,
        input: ActionInput::Ability,
        conditions: &[],
        effect: RuleEffect::Hold,
    },
];

static RULES: [CompatibilityRule; 8] = [
    action::RULES[0],
    action::RULES[1],
    action::RULES[2],
    action::RULES[3],
    action::RULES[4],
    WATER_RULES[0],
    WATER_RULES[1],
    WATER_RULES[2],
];

/// A gliding swim: full momentum, 4 m/s² of steering.
static GLIDE: [MovementRule; 1] = [MovementRule {
    action: ActionId::Swim,
    phase: Phase::Active,
    conditions: &[],
    movement: Movement {
        steering_percent: 100,
        momentum_percent: 100,
        max_acceleration_cms2: Some(400),
    },
}];

static WATER: ActionGraph = ActionGraph {
    rules: &RULES,
    movement: &GLIDE,
    ..action::BASELINE
};

/// Dry ground east of the pool, 6 m from the wolf.
const ASHORE: [f32; 3] = [1_000.0, 90.0, 0.0];

/// A 3 m deep pool around the spawn and a neutral wolf on dry ground.
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
            vec![arena::spawn(1, WOLF, [1_000, -600])],
            Vec::new(),
            GRAVEYARD,
        )
        .unwrap()
        .with_creature_abilities(Default::default())
        .unwrap()
        .with_waters(vec![WaterVolume {
            name: "pool".into(),
            centre: [0, 0],
            radii: [400, 400],
            surface: 300,
            air_pockets: Vec::new(),
        }])
        .unwrap(),
    )
}

struct Swimmer {
    zone: ZoneSimulation,
    graph: &'static ActionGraph,
    sequence: u32,
}

impl Swimmer {
    /// A level-2 swimmer of `class` with a full resource, running `graph`.
    fn new(class: PlayerClass, graph: &'static ActionGraph) -> Self {
        let mut zone = ZoneSimulation::with_content(ZoneId::new(9), content()).unwrap();
        zone.add_player(1).unwrap();
        let mut swimmer = Self {
            zone,
            graph,
            sequence: 0,
        };
        swimmer.send(ZoneCommand::ChooseClass {
            class: class.code(),
            sex: 0,
        });
        swimmer.send(arena::stand(SOUTH));
        swimmer.ticks(mmorpg_core::golden::t30(20));
        swimmer.restore(|state| {
            let player = &mut state.players[0].combat;
            player.level = 2;
            player.health = mmorpg_core::unit::player_max_health(2);
            player.abilities.resource = 100;
        });
        swimmer
    }

    /// Restores the zone from its edited snapshot, running the same graph.
    fn restore(&mut self, edit: impl FnOnce(&mut mmorpg_core::CanonicalZoneSnapshot)) {
        let mut state = self.zone.snapshot().unwrap();
        edit(&mut state);
        self.zone = ZoneSimulation::from_snapshot(state, Arc::clone(self.zone.content())).unwrap();
        self.zone.set_action_graph(self.graph).unwrap();
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

    fn arbitrate(&self, input: ActionInput) -> action::Arbitration {
        self.zone.arbitrate_input(1, input).unwrap()
    }

    fn use_ability(&mut self, ability: mmorpg_core::AbilityId) {
        self.send(ZoneCommand::UseAbility {
            ability: ability.get(),
            target: Some(EntityRef::Creature(CreatureId::new(1))),
        });
    }
}

fn errors(events: &[ZoneEvent]) -> Vec<ErrorCode> {
    events
        .iter()
        .filter_map(|event| match *event {
            ZoneEvent::Error { code, .. } => Some(code),
            _ => None,
        })
        .collect()
}

#[test]
fn a_swimmer_runs_the_swim_action_and_production_still_attacks() {
    let mut warden = Swimmer::new(PlayerClass::Warden, &action::BASELINE);
    let movement = warden.movement();
    assert_eq!(movement.action, Some(ActionId::Swim));
    assert_eq!(movement.phase, Some(Phase::Active));
    // The production graph authors nothing for water.
    let inspection = warden.inspect();
    assert!(inspection.rules.is_empty());
    assert!(inspection.movement.sources.is_empty());
    for input in [
        ActionInput::WardenMelee,
        ActionInput::ShieldBash,
        ActionInput::Ability,
    ] {
        assert_eq!(
            warden.arbitrate(input).outcome,
            Outcome::Proceed(Candidate::Direct),
            "{input:?}"
        );
    }
    warden.use_ability(ids::SHIELD_BASH);
    let events = warden.tick();
    assert!(!events.is_empty());
    assert!(!errors(&events).contains(&ErrorCode::NotWhileSwimming));
}

#[test]
fn water_rules_refuse_melee_and_report_until_ashore() {
    let mut warden = Swimmer::new(PlayerClass::Warden, &WATER);
    assert_eq!(warden.movement().action, Some(ActionId::Swim));
    let arbitration = warden.arbitrate(ActionInput::ShieldBash);
    assert_eq!(
        arbitration.outcome,
        Outcome::Refuse(RuleId::SwimmingRefuses)
    );
    assert!(
        arbitration
            .candidates
            .iter()
            .all(|(_, verdict)| *verdict == Verdict::RefusedBy(RuleId::SwimmingRefuses))
    );
    let inspection = warden.inspect();
    let refusal = inspection
        .rules
        .iter()
        .find(|rule| rule.input == ActionInput::ShieldBash)
        .unwrap();
    assert_eq!(refusal.effect, RuleEffect::Refuse);
    assert_eq!(refusal.released_at, None);
    // The swim's authored glide is the running movement.
    assert_eq!(inspection.movement.sources.len(), 1);
    assert_eq!(inspection.movement.sources[0].action, ActionId::Swim);

    warden.use_ability(ids::SHIELD_BASH);
    assert_eq!(errors(&warden.tick()), [ErrorCode::NotWhileSwimming]);
    // Refused, not queued: nothing resolves later.
    assert!(errors(&warden.tick()).is_empty());

    // Ashore the same graph allows the Bash again.
    warden.restore(|state| state.players[0].position = ASHORE);
    warden.ticks(mmorpg_core::golden::t30(30));
    assert_eq!(warden.movement().action, None);
    assert_eq!(
        warden.arbitrate(ActionInput::ShieldBash).outcome,
        Outcome::Proceed(Candidate::Direct)
    );
}

#[test]
fn a_held_ability_waits_in_water_and_resolves_ashore() {
    let mut ranger = Swimmer::new(PlayerClass::Ranger, &WATER);
    ranger.use_ability(ids::AIMED_SHOT);
    for _ in 0..mmorpg_core::golden::t30(10) {
        let events = ranger.tick();
        assert!(
            !events.iter().any(|event| matches!(
                event,
                ZoneEvent::AbilityUsed { .. }
                    | ZoneEvent::CastStarted { .. }
                    | ZoneEvent::Error { .. }
            )),
            "{events:?}"
        );
    }
    assert_eq!(
        ranger.arbitrate(ActionInput::Ability).outcome,
        Outcome::WaitForRule(RuleId::SwimmingHolds)
    );
    // The held shot survives a restore and resolves once the swim ends.
    ranger.restore(|state| state.players[0].position = ASHORE);
    assert_eq!(ranger.movement().action, None);
    let events = ranger.tick();
    assert!(
        events.iter().any(|event| matches!(
            event,
            ZoneEvent::AbilityUsed { .. } | ZoneEvent::CastStarted { .. }
        )),
        "{events:?}"
    );
}

#[test]
fn only_a_swim_refuses_and_only_attacks() {
    let refuse = |during, input| ActionGraph {
        rules: Box::leak(Box::new([CompatibilityRule {
            id: RuleId::SwimmingRefuses,
            during,
            input,
            conditions: &[],
            effect: RuleEffect::Refuse,
        }])),
        interruptions: &[],
        ..action::BASELINE
    };
    assert_eq!(
        refuse(ActionId::Landing, ActionInput::WardenMelee).validate(),
        Err(GraphError::UnsupportedRule(RuleId::SwimmingRefuses))
    );
    assert_eq!(
        refuse(ActionId::Swim, ActionInput::GuardProtection).validate(),
        Err(GraphError::UnsupportedRule(RuleId::SwimmingRefuses))
    );
    assert_eq!(
        refuse(ActionId::Swim, ActionInput::Jump).validate(),
        Err(GraphError::SameRegion(ActionId::Swim))
    );
    assert_eq!(
        refuse(ActionId::Swim, ActionInput::Ability).validate(),
        Ok(())
    );
    // A swim holds without a recovery phase; an airborne phase cannot.
    let hold = |during| ActionGraph {
        rules: Box::leak(Box::new([CompatibilityRule {
            id: RuleId::SwimmingHolds,
            during,
            input: ActionInput::Ability,
            conditions: &[],
            effect: RuleEffect::Hold,
        }])),
        interruptions: &[],
        ..action::BASELINE
    };
    assert_eq!(hold(ActionId::Swim).validate(), Ok(()));
    assert_eq!(
        hold(ActionId::Airborne).validate(),
        Err(GraphError::NoRecovery(RuleId::SwimmingHolds))
    );
    assert_eq!(WATER.validate(), Ok(()));
}
