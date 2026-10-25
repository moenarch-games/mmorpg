//! Every playable class on the one action graph (#380, PR 3, Atlas 5.7.5,
//! 5.7.6): shared baseline definitions, class-, weapon- and ability-specific
//! overrides with their resolved origin, and the Ranger's and Arcanist's
//! action paths (shots, casting and moving, a stun interrupting a cast)
//! inspected with baseline parity. Overrides run only through validated test
//! configurations; the production graph has none.
mod support;

use std::sync::Arc;

use mmorpg_core::ability::ids;
use mmorpg_core::action::{
    self, ActionGraph, ActionId, ActionInput, Candidate, CompatibilityRule, Consequence, Context,
    Effective, GraphError, Interrupted, InterruptionReport, InterruptionRule, Key, Origin, Outcome,
    Override, Phase, Region, RuleEffect, RuleId, Scope, Setting, Verdict,
};
use mmorpg_core::{
    AbilityId, CanonicalPlayerSnapshot, CreatureBehaviour, CreatureId, CreatureTemplate,
    EQUIPMENT_SLOTS, EntityRef, Equipment, ItemId, PlayerClass, PlayerCombatPolicy, ZoneAreas,
    ZoneCommand, ZoneContent, ZoneEvent, ZoneId, ZoneSimulation,
};
use support::arena::{self, GRAVEYARD, WOLF};

const SOUTH: u16 = 32_768;
const WOLF_ID: EntityRef = EntityRef::Creature(CreatureId::new(1));
const GCD: u16 = mmorpg_core::GLOBAL_COOLDOWN_TICKS;
/// The Worn Dagger, a main-hand catalog item.
const DAGGER: ItemId = ItemId::new(2);

/// Ranger timing for a test: a class-wide 1 s global cooldown, 1.25 s with
/// a dagger in hand, and 0.75 s after Aimed Shot.
static RANGER_TUNED: ActionGraph = ActionGraph {
    overrides: &[
        Override {
            scope: Scope::Class(PlayerClass::Ranger),
            setting: Setting::Recovery {
                action: ActionId::GlobalCooldown,
                ms: 1_000,
            },
        },
        Override {
            scope: Scope::Weapon(DAGGER),
            setting: Setting::Recovery {
                action: ActionId::GlobalCooldown,
                ms: 1_250,
            },
        },
        Override {
            scope: Scope::Ability(ids::AIMED_SHOT),
            setting: Setting::Recovery {
                action: ActionId::GlobalCooldown,
                ms: 750,
            },
        },
    ],
    ..action::BASELINE
};

/// Firebolt can be cast while moving; every other cast still cannot.
static FIREBOLT_ON_THE_MOVE: ActionGraph = ActionGraph {
    overrides: &[Override {
        scope: Scope::Ability(ids::FIREBOLT),
        setting: Setting::Rule {
            rule: RuleId::MovementCancelsCast,
            applies: false,
        },
    }],
    ..action::BASELINE
};

/// A stun preserves a running cast.
static UNSHAKEN: ActionGraph = ActionGraph {
    interruptions: &[InterruptionRule {
        reaction: ActionId::Stun,
        interrupted: Interrupted::Action(ActionId::Cast),
        phase: None,
        consequence: Consequence::Preserve,
    }],
    ..action::BASELINE
};

fn content() -> Arc<ZoneContent> {
    let template = CreatureTemplate {
        behaviour: CreatureBehaviour::Neutral,
        ..arena::wolf(50_000, [1, 1])
    };
    Arc::new(
        ZoneContent::new(
            arena::ground(),
            ZoneAreas::default(),
            vec![template],
            vec![arena::spawn(1, WOLF, [0, -600])],
            Vec::new(),
            GRAVEYARD,
        )
        .unwrap()
        .with_creature_abilities(Default::default())
        .unwrap()
        // A hostile Warden beside the hero can stun it.
        .with_player_combat(PlayerCombatPolicy::FreeForAll),
    )
}

/// A level-6 hero of one class with a full resource, running a graph, and
/// a hostile level-2 Warden at its side.
struct Hero {
    zone: ZoneSimulation,
    content: Arc<ZoneContent>,
    graph: &'static ActionGraph,
    sequence: u32,
    warden_sequence: u32,
}

impl Hero {
    fn new(class: PlayerClass, graph: &'static ActionGraph) -> Self {
        let content = content();
        let mut zone = ZoneSimulation::with_content(ZoneId::new(9), Arc::clone(&content)).unwrap();
        zone.add_player(1).unwrap();
        zone.add_player(2).unwrap();
        let mut hero = Self {
            zone,
            content,
            graph,
            sequence: 0,
            warden_sequence: 1,
        };
        hero.zone
            .apply_command(2, 1, ZoneCommand::ChooseClass { class: 0, sex: 0 })
            .unwrap();
        hero.send(ZoneCommand::ChooseClass {
            class: class.code(),
            sex: 0,
        });
        hero.send(arena::stand(SOUTH));
        for _ in 0..mmorpg_core::golden::t30(20) {
            hero.tick();
        }
        hero.restore(|players| {
            for player in players.iter_mut() {
                player.combat.level = 6;
                player.combat.health = mmorpg_core::unit::player_max_health(6);
                player.combat.abilities.resource = 100;
            }
            let [x, y, z] = players[0].position;
            players[1].position = [x + 120.0, y, z];
        });
        hero
    }

    /// Edits the canonical players and restores the zone, keeping its graph.
    fn restore(&mut self, edit: impl FnOnce(&mut [CanonicalPlayerSnapshot])) {
        let mut state = self.zone.snapshot().unwrap();
        edit(&mut state.players);
        self.zone = ZoneSimulation::from_snapshot(state, Arc::clone(&self.content)).unwrap();
        self.zone.set_action_graph(self.graph).unwrap();
    }

    fn wield(&mut self, item: ItemId) {
        let mut slots = [None; EQUIPMENT_SLOTS];
        slots[0] = Some(item);
        self.restore(|players| players[0].equipment = Equipment::from_slots(slots).unwrap());
    }

    fn send(&mut self, command: ZoneCommand) {
        self.sequence += 1;
        self.zone.apply_command(1, self.sequence, command).unwrap();
    }

    fn tick(&mut self) -> Vec<ZoneEvent> {
        self.zone.advance_tick().unwrap();
        self.zone.snapshot_for_player(1).unwrap().events
    }

    fn ticks(&mut self, count: u16) {
        for _ in 0..count {
            self.tick();
        }
    }

    fn use_ability(&mut self, ability: AbilityId, target: Option<EntityRef>) -> Vec<ZoneEvent> {
        self.send(ZoneCommand::UseAbility {
            ability: ability.get(),
            target,
        });
        self.tick()
    }

    /// The hostile Warden's Shield Bash on the hero.
    fn bashed(&mut self) -> Vec<ZoneEvent> {
        self.warden_sequence += 1;
        self.zone
            .apply_command(
                2,
                self.warden_sequence,
                ZoneCommand::UseAbility {
                    ability: ids::SHIELD_BASH.get(),
                    target: Some(EntityRef::Player(1)),
                },
            )
            .unwrap();
        self.tick()
    }

    fn inspect(&self) -> action::ActionInspection {
        self.zone.action_inspection(1).unwrap()
    }

    fn offense(&self) -> action::RegionReport {
        self.inspect().regions[Region::Offense as usize]
    }

    fn arbitrate(&self, input: ActionInput) -> action::Arbitration {
        self.zone.arbitrate_input(1, input).unwrap()
    }

    fn effective(&self, key: Key) -> Effective {
        self.inspect()
            .effective
            .into_iter()
            .find(|value| value.setting.key() == key)
            .unwrap()
    }

    fn global_cooldown(&self) -> u16 {
        self.zone.snapshot().unwrap().players[0]
            .combat
            .abilities
            .global_cooldown
    }

    fn casting(&self) -> bool {
        self.zone.snapshot().unwrap().players[0]
            .combat
            .abilities
            .cast
            .is_some()
    }
}

fn interrupted(events: &[ZoneEvent], by: Option<EntityRef>, ability: AbilityId) -> bool {
    events.iter().any(|event| {
        *event
            == ZoneEvent::Interrupted {
                source: by,
                target: EntityRef::Player(1),
                ability,
            }
    })
}

fn used(events: &[ZoneEvent], ability: AbilityId) -> bool {
    events.iter().any(
        |event| matches!(event, ZoneEvent::AbilityUsed { ability: used, .. } if *used == ability),
    )
}

#[test]
fn a_ranger_shoots_on_the_move_through_the_shared_global_cooldown() {
    let mut ranger = Hero::new(PlayerClass::Ranger, &action::BASELINE);
    ranger.send(arena::walk(SOUTH));
    ranger.tick();
    let events = ranger.use_ability(ids::AIMED_SHOT, Some(WOLF_ID));
    assert!(used(&events, ids::AIMED_SHOT), "{events:?}");

    // A shot never casts: moving cancels nothing, and the offense region
    // runs the shared global cooldown.
    let offense = ranger.offense();
    assert_eq!(offense.action, Some(ActionId::GlobalCooldown));
    assert_eq!(offense.phase, Some(Phase::Recovery));
    assert_eq!(offense.recovery_left, Some(GCD - 1));
    assert_eq!(ranger.global_cooldown(), GCD - 1);
    let inspection = ranger.inspect();
    assert!(inspection.rules.is_empty());
    assert!(inspection.interruptions.is_empty());
    // A Ranger has no landing recovery or guard; the Warden's regions idle.
    assert_eq!(inspection.regions[Region::Movement as usize].action, None);
    assert_eq!(inspection.regions[Region::Defense as usize].action, None);
    for input in [ActionInput::Move, ActionInput::Jump, ActionInput::Ability] {
        let arbitration = ranger.arbitrate(input);
        assert_eq!(arbitration.outcome, Outcome::Proceed(Candidate::Direct));
        assert_eq!(arbitration.candidates, [(Candidate::Direct, Verdict::Won)]);
    }

    // Every effective value is the shared baseline's.
    assert!(
        inspection
            .effective
            .iter()
            .all(|value| value.origin == Origin::Baseline)
    );
    assert_eq!(
        ranger.effective(Key::Recovery(ActionId::GlobalCooldown)),
        Effective {
            setting: Setting::Recovery {
                action: ActionId::GlobalCooldown,
                ms: 1_500,
            },
            origin: Origin::Baseline,
        }
    );

    // The cooldown ends at the inspected tick; the next shot is ordinary.
    ranger.ticks(GCD - 2);
    assert_eq!(ranger.offense().recovery_left, Some(1));
    ranger.tick();
    assert_eq!(ranger.offense().action, None);
    let events = ranger.use_ability(ids::SERPENT_STING, Some(WOLF_ID));
    assert!(used(&events, ids::SERPENT_STING), "{events:?}");
}

#[test]
fn ranger_overrides_resolve_ability_over_weapon_over_class() {
    let mut ranger = Hero::new(PlayerClass::Ranger, &RANGER_TUNED);
    let class = Origin::Override(Scope::Class(PlayerClass::Ranger));
    assert_eq!(
        ranger.effective(Key::Recovery(ActionId::GlobalCooldown)),
        Effective {
            setting: Setting::Recovery {
                action: ActionId::GlobalCooldown,
                ms: 1_000,
            },
            origin: class,
        }
    );
    // The class override applies to a Serpent Sting.
    ranger.use_ability(ids::SERPENT_STING, Some(WOLF_ID));
    assert_eq!(
        ranger.global_cooldown(),
        mmorpg_core::ticks_from_ms(1_000) - 1
    );
    ranger.ticks(mmorpg_core::ticks_from_ms(1_000));
    // Aimed Shot's own override outranks the class's.
    ranger.use_ability(ids::AIMED_SHOT, Some(WOLF_ID));
    assert_eq!(
        ranger.global_cooldown(),
        mmorpg_core::ticks_from_ms(750) - 1
    );
    ranger.ticks(mmorpg_core::ticks_from_ms(750));

    // A dagger in hand outranks the class, but not the ability.
    ranger.wield(DAGGER);
    assert_eq!(
        ranger
            .effective(Key::Recovery(ActionId::GlobalCooldown))
            .origin,
        Origin::Override(Scope::Weapon(DAGGER))
    );
    ranger.use_ability(ids::CONCUSSIVE_SHOT, Some(WOLF_ID));
    assert_eq!(
        ranger.global_cooldown(),
        mmorpg_core::ticks_from_ms(1_250) - 1
    );
    let graph = &RANGER_TUNED;
    let context = Context {
        class: Some(PlayerClass::Ranger),
        weapon: Some(DAGGER),
        ability: Some(ids::AIMED_SHOT),
    };
    assert_eq!(
        graph
            .resolve(Key::Recovery(ActionId::GlobalCooldown), context)
            .map(|value| value.origin),
        Some(Origin::Override(Scope::Ability(ids::AIMED_SHOT)))
    );
    // Other classes keep the baseline.
    let arcanist = Context {
        class: Some(PlayerClass::Arcanist),
        ..Context::default()
    };
    assert_eq!(
        graph
            .resolve(Key::Recovery(ActionId::GlobalCooldown), arcanist)
            .map(|value| value.origin),
        Some(Origin::Baseline)
    );
}

#[test]
fn an_arcanist_cast_is_cancelled_by_moving_at_the_inspected_ticks() {
    let mut arcanist = Hero::new(PlayerClass::Arcanist, &action::BASELINE);
    let cast = ids::FIREBOLT;
    let cast_ticks = mmorpg_core::ticks_from_ms(2_000);
    arcanist.use_ability(cast, Some(WOLF_ID));
    let tick = arcanist.inspect().tick;
    let offense = arcanist.offense();
    assert_eq!(offense.action, Some(ActionId::Cast));
    assert_eq!(offense.phase, Some(Phase::Active));
    assert_eq!(offense.ability, Some(cast));
    assert_eq!(offense.active_left, Some(cast_ticks - 1));

    // Moving or jumping would cancel the cast until it completes.
    let rules = arcanist.inspect().rules;
    assert_eq!(rules.len(), 2);
    for (rule, input) in rules.iter().zip([ActionInput::Move, ActionInput::Jump]) {
        assert_eq!(rule.rule, RuleId::MovementCancelsCast);
        assert_eq!(rule.input, input);
        assert_eq!(rule.effect, RuleEffect::Cancel);
        assert_eq!(rule.released_at, tick + u64::from(cast_ticks));
        assert_eq!(rule.origin, Origin::Baseline);
    }
    let arbitration = arcanist.arbitrate(ActionInput::Move);
    assert_eq!(
        arbitration.outcome,
        Outcome::Cancel(RuleId::MovementCancelsCast)
    );
    assert_eq!(arbitration.candidates, [(Candidate::Direct, Verdict::Won)]);
    assert_eq!(
        arcanist.effective(Key::Rule(RuleId::MovementCancelsCast)),
        Effective {
            setting: Setting::Rule {
                rule: RuleId::MovementCancelsCast,
                applies: true,
            },
            origin: Origin::Baseline,
        }
    );

    arcanist.send(arena::walk(SOUTH));
    let events = arcanist.tick();
    assert!(interrupted(&events, None, cast), "{events:?}");
    assert!(!arcanist.casting());
    // The global cooldown the cast started keeps running.
    assert_eq!(arcanist.offense().action, Some(ActionId::GlobalCooldown));
    assert!(arcanist.inspect().rules.is_empty());
}

#[test]
fn a_standing_arcanist_cast_completes_at_the_inspected_release() {
    let mut arcanist = Hero::new(PlayerClass::Arcanist, &action::BASELINE);
    arcanist.use_ability(ids::FIREBOLT, Some(WOLF_ID));
    let released = arcanist.inspect().rules[0].released_at;
    while arcanist.inspect().tick + 2 < released {
        arcanist.tick();
    }
    // The last cancellable tick is also the tick the cast completes.
    assert_eq!(arcanist.offense().active_left, Some(1));
    arcanist.tick();
    assert_eq!(arcanist.inspect().tick + 1, released);
    assert!(!arcanist.casting());
    assert_ne!(arcanist.offense().action, Some(ActionId::Cast));
}

#[test]
fn an_incoming_stun_interrupts_an_arcanist_cast() {
    let mut arcanist = Hero::new(PlayerClass::Arcanist, &action::BASELINE);
    arcanist.use_ability(ids::FIREBOLT, Some(WOLF_ID));
    arcanist.tick();
    assert!(arcanist.casting());
    let events = arcanist.bashed();
    assert!(
        interrupted(&events, Some(EntityRef::Player(2)), ids::FIREBOLT),
        "{events:?}"
    );
    assert!(!arcanist.casting());
    let inspection = arcanist.inspect();
    assert_eq!(
        inspection.regions[Region::Reaction as usize].action,
        Some(ActionId::Stun)
    );
    assert_ne!(
        inspection.regions[Region::Offense as usize].action,
        Some(ActionId::Cast)
    );

    // A configuration in which a stun preserves the cast keeps it running
    // and says so.
    let mut unshaken = Hero::new(PlayerClass::Arcanist, &UNSHAKEN);
    unshaken.use_ability(ids::FIREBOLT, Some(WOLF_ID));
    unshaken.tick();
    let events = unshaken.bashed();
    assert!(
        !interrupted(&events, Some(EntityRef::Player(2)), ids::FIREBOLT),
        "{events:?}"
    );
    assert!(unshaken.casting());
    assert_eq!(
        unshaken.inspect().interruptions,
        [InterruptionReport {
            interrupted: Interrupted::Action(ActionId::Cast),
            reaction: ActionId::Stun,
            consequence: Consequence::Preserve,
            authored: true,
        }]
    );
}

#[test]
fn an_ability_override_lets_firebolt_alone_be_cast_on_the_move() {
    let mut arcanist = Hero::new(PlayerClass::Arcanist, &FIREBOLT_ON_THE_MOVE);
    arcanist.use_ability(ids::FIREBOLT, Some(WOLF_ID));
    assert!(arcanist.inspect().rules.is_empty());
    assert_eq!(
        arcanist.effective(Key::Rule(RuleId::MovementCancelsCast)),
        Effective {
            setting: Setting::Rule {
                rule: RuleId::MovementCancelsCast,
                applies: false,
            },
            origin: Origin::Override(Scope::Ability(ids::FIREBOLT)),
        }
    );
    assert_eq!(
        arcanist.arbitrate(ActionInput::Move).outcome,
        Outcome::Proceed(Candidate::Direct)
    );
    arcanist.send(arena::walk(SOUTH));
    let mut completed = false;
    for _ in 0..mmorpg_core::ticks_from_ms(2_000) {
        let events = arcanist.tick();
        assert!(!interrupted(&events, None, ids::FIREBOLT), "{events:?}");
        completed |= !arcanist.casting();
    }
    assert!(completed, "the moving Firebolt completes");

    // Fire Blast keeps the shared rule.
    arcanist.send(arena::stand(SOUTH));
    arcanist.ticks(GCD);
    arcanist.use_ability(ids::FIRE_BLAST, Some(WOLF_ID));
    assert_eq!(
        arcanist
            .effective(Key::Rule(RuleId::MovementCancelsCast))
            .origin,
        Origin::Baseline
    );
    arcanist.send(arena::walk(SOUTH));
    let events = arcanist.tick();
    assert!(interrupted(&events, None, ids::FIRE_BLAST), "{events:?}");
}

#[test]
fn every_caster_class_casts_on_the_same_model() {
    for (class, spell) in [
        (PlayerClass::Arcanist, ids::FIREBOLT),
        (PlayerClass::Necromancer, ids::SHADOW_BOLT),
        (PlayerClass::Cleric, ids::SMITE),
        (PlayerClass::Druid, ids::WRATH),
    ] {
        let mut hero = Hero::new(class, &action::BASELINE);
        hero.use_ability(spell, Some(WOLF_ID));
        let offense = hero.offense();
        assert_eq!(offense.action, Some(ActionId::Cast), "{class:?}");
        assert_eq!(offense.ability, Some(spell), "{class:?}");
        assert_eq!(
            hero.arbitrate(ActionInput::Jump).outcome,
            Outcome::Cancel(RuleId::MovementCancelsCast),
            "{class:?}"
        );
        hero.send(ZoneCommand::Jump);
        let events = hero.tick();
        assert!(interrupted(&events, None, spell), "{class:?}: {events:?}");
        assert!(!hero.casting(), "{class:?}");
    }
}

#[test]
fn conflicting_and_unsupported_overrides_are_refused() {
    action::validate().unwrap();
    RANGER_TUNED.validate().unwrap();
    FIREBOLT_ON_THE_MOVE.validate().unwrap();
    UNSHAKEN.validate().unwrap();
    let recovery = |scope, action, ms| Override {
        scope,
        setting: Setting::Recovery { action, ms },
    };
    let ranger = Scope::Class(PlayerClass::Ranger);
    let refused = |overrides: &'static [Override]| {
        ActionGraph {
            overrides,
            ..action::BASELINE
        }
        .validate()
    };
    static CONFLICT: [Override; 2] = [
        Override {
            scope: Scope::Class(PlayerClass::Ranger),
            setting: Setting::Recovery {
                action: ActionId::GlobalCooldown,
                ms: 1_000,
            },
        },
        Override {
            scope: Scope::Class(PlayerClass::Ranger),
            setting: Setting::Recovery {
                action: ActionId::GlobalCooldown,
                ms: 1_200,
            },
        },
    ];
    assert_eq!(
        refused(&CONFLICT),
        Err(GraphError::ConflictingOverride(
            ranger,
            Key::Recovery(ActionId::GlobalCooldown)
        ))
    );
    let leak = |entry: Override| -> &'static [Override] { Box::leak(Box::new([entry])) };
    assert_eq!(
        refused(leak(recovery(ranger, ActionId::Landing, 100))),
        Err(GraphError::UnsupportedOverride(Key::Recovery(
            ActionId::Landing
        )))
    );
    // Canonical snapshots bound the global cooldown by the baseline.
    assert_eq!(
        refused(leak(recovery(ranger, ActionId::GlobalCooldown, 2_000))),
        Err(GraphError::BadOverride(Key::Recovery(
            ActionId::GlobalCooldown
        )))
    );
    // Muck Bolt is a creature ability; Torn Fur is no weapon.
    for scope in [
        Scope::Ability(ids::MUCK_BOLT),
        Scope::Weapon(ItemId::new(1)),
    ] {
        assert_eq!(
            refused(leak(recovery(scope, ActionId::GlobalCooldown, 1_000))),
            Err(GraphError::BadScope(scope))
        );
    }
    static SLASH_CANCELS: [CompatibilityRule; 1] = [CompatibilityRule {
        id: RuleId::MovementCancelsCast,
        during: ActionId::DirectedSlash,
        input: ActionInput::Move,
        conditions: &[],
        effect: RuleEffect::Cancel,
    }];
    assert_eq!(
        ActionGraph {
            rules: &SLASH_CANCELS,
            interruptions: &[],
            ..action::BASELINE
        }
        .validate(),
        Err(GraphError::UnsupportedRule(RuleId::MovementCancelsCast))
    );
    static SUSPENDED_CAST: [InterruptionRule; 1] = [InterruptionRule {
        reaction: ActionId::Stun,
        interrupted: Interrupted::Action(ActionId::Cast),
        phase: None,
        consequence: Consequence::Suspend,
    }];
    assert_eq!(
        ActionGraph {
            interruptions: &SUSPENDED_CAST,
            ..action::BASELINE
        }
        .validate(),
        Err(GraphError::UnsupportedInterruption(Interrupted::Action(
            ActionId::Cast
        )))
    );
    let mut zone = ZoneSimulation::new(ZoneId::new(1));
    assert!(
        zone.set_action_graph(Box::leak(Box::new(ActionGraph {
            overrides: &CONFLICT,
            ..action::BASELINE
        })))
        .is_err()
    );
    assert_eq!(zone.action_graph(), &action::BASELINE);
}
