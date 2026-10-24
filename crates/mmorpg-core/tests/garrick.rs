//! Shared outdoor boss rules (#276, Atlas 5.1.72–5.1.80) on a small arena: a boss and two
//! bandits of its family, three players. The boss locks its scaling when combat starts, every
//! meaningful contributor with the kill quest earns the objective, the camp rallies on its
//! death, its corpse stays until it returns, it resets fully after an evade, and it telegraphs
//! its Redbrand Whirl.
mod support;

use std::sync::Arc;

use mmorpg_core::ability::ids;
use mmorpg_core::{
    BossRules, CanonicalCreatureSnapshot, CreatureAi, CreatureBehaviour, CreatureFamily,
    CreatureId, CreatureLife, CreatureTemplate, CreatureTemplateId, EntityRef, Npc, NpcId, NpcRole,
    Quest, QuestEntry, QuestId, QuestObjective, QuestRewards, ZoneCommand, ZoneContent, ZoneEvent,
    ZoneId, ZoneSimulation,
};
use support::arena;

const BOSS_TEMPLATE: CreatureTemplateId = CreatureTemplateId::new(5);
const BANDIT_TEMPLATE: CreatureTemplateId = CreatureTemplateId::new(6);
const BOSS: CreatureId = CreatureId::new(1);
const FIGHTING_BANDIT: CreatureId = CreatureId::new(2);
const IDLE_BANDIT: CreatureId = CreatureId::new(3);
const GIVER: NpcId = NpcId::new(1);
const HUNT: QuestId = QuestId::new(1);
const BOSS_HEALTH: u32 = 1_000;
/// The boss's spawn: far enough from both bandits that neither assists the other.
const BOSS_AT: [i32; 2] = [0, 2_000];
const BANDIT_AT: [i32; 2] = [2_000, 2_000];
const IDLE_BANDIT_AT: [i32; 2] = [-1_500, 2_000];
const RALLY: [i32; 2] = [0, 3_000];

fn template(id: CreatureTemplateId, health: u32) -> CreatureTemplate {
    CreatureTemplate {
        id,
        name: format!("Redbrand {}", id.get()),
        family: CreatureFamily::Redbrand,
        behaviour: CreatureBehaviour::Neutral,
        min_level: 5,
        max_level: 5,
        health,
        damage: [1, 1],
        swing_ticks: mmorpg_core::golden::t30(90) as _,
        half_extents: [40, 100, 40],
        ..arena::wolf(health, [1, 1])
    }
}

fn content(whirl: bool) -> Arc<ZoneContent> {
    let base = ZoneContent::new(
        arena::ground(),
        mmorpg_core::ZoneAreas::default(),
        vec![
            template(BOSS_TEMPLATE, BOSS_HEALTH),
            template(BANDIT_TEMPLATE, 5_000),
        ],
        vec![
            arena::spawn(1, BOSS_TEMPLATE, BOSS_AT),
            arena::spawn(2, BANDIT_TEMPLATE, BANDIT_AT),
            arena::spawn(3, BANDIT_TEMPLATE, IDLE_BANDIT_AT),
        ],
        vec![Npc {
            role: NpcRole::QuestGiver,
            ..arena::npc(GIVER.get(), [300, -300])
        }],
        arena::GRAVEYARD,
    )
    .unwrap();
    let abilities = if whirl {
        vec![(BOSS_TEMPLATE, ids::REDBRAND_WHIRL)]
    } else {
        Vec::new()
    };
    Arc::new(
        base.with_creature_abilities(abilities)
            .unwrap()
            .with_bosses(vec![BossRules {
                template: BOSS_TEMPLATE,
                health_percent_per_extra: 60,
                damage_percent_per_extra: 10,
                engage_radius: 2_500,
                reaction_radius: 3_000,
                reaction_ticks: mmorpg_core::golden::t30(60) as _,
                rally_point: RALLY,
            }])
            .unwrap()
            .with_quests(vec![Quest {
                id: HUNT,
                name: "Hunt the boss".into(),
                text: "Bring him down.".into(),
                giver: GIVER,
                ender: GIVER,
                prerequisite: None,
                objectives: vec![QuestObjective::Kill {
                    template: BOSS_TEMPLATE,
                    count: 1,
                }],
                rewards: QuestRewards::default(),
            }])
            .unwrap(),
    )
}

struct Run {
    zone: ZoneSimulation,
    sequence: u32,
    events: Vec<(u32, ZoneEvent)>,
}

impl Run {
    fn new(content: Arc<ZoneContent>, players: u32) -> Self {
        let mut zone = ZoneSimulation::with_content(ZoneId::new(9), content).unwrap();
        for player in 1..=players {
            zone.add_player(player).unwrap();
        }
        Self {
            zone,
            sequence: 0,
            events: Vec::new(),
        }
    }

    /// Edits the canonical state between ticks.
    fn edit(&mut self, change: impl FnOnce(&mut mmorpg_core::CanonicalZoneSnapshot)) {
        let mut state = self.zone.snapshot().unwrap();
        change(&mut state);
        self.zone = ZoneSimulation::from_snapshot(state, Arc::clone(self.zone.content())).unwrap();
    }

    /// Player `id` stands at feet `[x, z]`, with the kill quest when `quest`.
    fn place(&mut self, id: u32, at: [i32; 2], quest: bool) {
        self.edit(|state| {
            let player = state
                .players
                .iter_mut()
                .find(|player| player.player_id == id)
                .unwrap();
            player.position = [at[0] as f32, 90.0, at[1] as f32];
            if quest {
                player.quests.entries = vec![QuestEntry {
                    quest: HUNT,
                    progress: [0; 3],
                }];
            }
        });
    }

    fn attack(&mut self, player: u32, creature: CreatureId) {
        for command in [
            ZoneCommand::SelectTarget(Some(EntityRef::Creature(creature))),
            ZoneCommand::StartAttack,
        ] {
            self.sequence += 1;
            self.zone
                .apply_command(player, self.sequence, command)
                .unwrap();
        }
    }

    fn tick(&mut self) {
        self.zone.advance_tick().unwrap();
        for player in self.zone.snapshot().unwrap().players {
            let id = player.player_id;
            let events = self.zone.snapshot_for_player(id).unwrap().events;
            self.events
                .extend(events.into_iter().map(|event| (id, event)));
        }
    }

    fn until(&mut self, limit: usize, done: impl Fn(&Self) -> bool) {
        for _ in 0..limit {
            if done(self) {
                return;
            }
            self.tick();
        }
        assert!(done(self), "condition not reached in {limit} ticks");
    }

    fn creature(&self, id: CreatureId) -> CanonicalCreatureSnapshot {
        self.zone
            .snapshot()
            .unwrap()
            .creatures
            .into_iter()
            .find(|creature| creature.creature_id == id)
            .unwrap()
    }

    fn progress(&self, player: u32) -> u8 {
        self.zone
            .snapshot()
            .unwrap()
            .players
            .into_iter()
            .find(|state| state.player_id == player)
            .unwrap()
            .quests
            .entry(HUNT)
            .map_or(0, |entry| entry.progress[0])
    }
}

#[test]
fn the_boss_locks_scaling_at_combat_start_and_credits_every_contributor() {
    let mut run = Run::new(content(false), 3);
    // Player 2 already fights a bandit nearby; player 3 has the quest and joins the boss later;
    // player 1 pulls the boss without the quest.
    run.place(1, [BOSS_AT[0], BOSS_AT[1] - 120], false);
    run.place(2, [BANDIT_AT[0], BANDIT_AT[1] - 120], true);
    run.place(3, [BOSS_AT[0] + 120, BOSS_AT[1]], true);
    run.attack(2, FIGHTING_BANDIT);
    run.until(mmorpg_core::golden::t30(200) as _, |run| {
        run.creature(FIGHTING_BANDIT).ai == CreatureAi::Engaged
    });

    run.attack(1, BOSS);
    run.until(mmorpg_core::golden::t30(200) as _, |run| {
        run.creature(BOSS).encounter.is_some()
    });
    let boss = run.creature(BOSS);
    let encounter = boss.encounter.clone().unwrap();
    assert_eq!(
        encounter.participants,
        [1, 2],
        "the puller and the player fighting its camp"
    );
    assert!(
        boss.health > BOSS_HEALTH,
        "two participants lock 160% health ({})",
        boss.health
    );

    // A later arrival contributes but never rescales the attempt.
    run.attack(3, BOSS);
    run.until(mmorpg_core::golden::t30(300) as _, |run| {
        run.creature(BOSS)
            .encounter
            .is_some_and(|encounter| encounter.contributors.contains(&3))
    });
    assert_eq!(run.creature(BOSS).encounter.unwrap().participants, [1, 2]);

    // Finish him: the tapper (no quest) gets XP and loot, contributor 3 earns the objective,
    // player 2 (fought only the bandit) does not.
    run.edit(|state| {
        let boss = state
            .creatures
            .iter_mut()
            .find(|creature| creature.creature_id == BOSS)
            .unwrap();
        boss.health = 1;
    });
    run.until(mmorpg_core::golden::t30(300) as _, |run| {
        run.creature(BOSS).life != CreatureLife::Alive
    });
    assert_eq!(run.progress(3), 1, "contributor 3 earns the kill objective");
    assert_eq!(run.progress(2), 0, "player 2 never fought the boss");
    let corpse = run.creature(BOSS);
    assert_eq!(corpse.tapped_by, Some(1));
    assert!(corpse.encounter.is_none());

    // The idle bandit rallies toward the rally point; the fighting one keeps fighting.
    assert!(
        matches!(run.creature(IDLE_BANDIT).ai, CreatureAi::Rallying { .. }),
        "{:?}",
        run.creature(IDLE_BANDIT).ai
    );
    assert_eq!(run.creature(FIGHTING_BANDIT).ai, CreatureAi::Engaged);
    run.until(mmorpg_core::golden::t30(200) as _, |run| {
        matches!(run.creature(IDLE_BANDIT).ai, CreatureAi::Idle { .. })
    });

    // The corpse stays past the ordinary corpse window, without loot, until the boss returns.
    let CreatureLife::Corpse { died_at } = corpse.life else {
        panic!("the boss left a corpse");
    };
    let after_window = died_at + u64::from(mmorpg_core::unit::CORPSE_TICKS) + 5;
    run.until(mmorpg_core::golden::t30(2_000) as _, |run| {
        run.zone.current_tick() >= after_window
    });
    let lingering = run.creature(BOSS);
    assert!(matches!(lingering.life, CreatureLife::Corpse { .. }));
    assert!(lingering.loot.is_none());
    run.until(mmorpg_core::golden::t30(2_000) as _, |run| {
        run.creature(BOSS).life == CreatureLife::Alive
    });
    assert_eq!(run.creature(BOSS).health, BOSS_HEALTH);
}

#[test]
fn an_abandoned_attempt_resets_the_boss_to_its_unscaled_rest() {
    let mut run = Run::new(content(false), 2);
    run.place(1, [BOSS_AT[0], BOSS_AT[1] - 120], false);
    run.place(2, [BOSS_AT[0], BOSS_AT[1] + 120], false);
    run.attack(1, BOSS);
    run.attack(2, BOSS);
    run.until(mmorpg_core::golden::t30(200) as _, |run| {
        run.creature(BOSS).encounter.is_some()
    });
    // Everybody leaves: the attempt ends without a kill.
    assert!(run.zone.remove_player(1));
    assert!(run.zone.remove_player(2));
    // It evades home (here at once: it never left), dropping the attempt's profile.
    run.until(mmorpg_core::golden::t30(600) as _, |run| {
        let boss = run.creature(BOSS);
        boss.encounter.is_none() && matches!(boss.ai, CreatureAi::Idle { .. })
    });
    assert_eq!(run.creature(BOSS).health, BOSS_HEALTH);
}

#[test]
fn the_boss_telegraphs_its_whirl_before_it_lands() {
    let mut run = Run::new(content(true), 1);
    run.place(1, [BOSS_AT[0], BOSS_AT[1] - 120], false);
    run.attack(1, BOSS);
    let whirl = ids::REDBRAND_WHIRL;
    run.until(mmorpg_core::golden::t30(1_200) as _, |run| {
        run.events.iter().any(|(_, event)| {
            matches!(event, ZoneEvent::AbilityUsed { ability, .. } if *ability == whirl)
        })
    });
    let started = run
        .events
        .iter()
        .position(|(_, event)| {
            matches!(event, ZoneEvent::CastStarted { ability, ticks, .. } if *ability == whirl && u64::from(*ticks) == mmorpg_core::golden::t30(45))
        })
        .expect("a 1.5 s wind-up precedes the whirl");
    let used = run
        .events
        .iter()
        .position(|(_, event)| {
            matches!(event, ZoneEvent::AbilityUsed { ability, .. } if *ability == whirl)
        })
        .unwrap();
    assert!(started < used);
    assert!(
        run.events[used..].iter().any(|(_, event)| matches!(
            event,
            ZoneEvent::DamageTaken {
                source: EntityRef::Creature(BOSS),
                ..
            }
        )),
        "standing in the ring hurts"
    );
}
