//! Quests through the public zone API (#25): content validation and
//! identity, accept/abandon/turn-in with every refusal leaving state
//! unchanged, kill, collect, talk and explore progress, quest-only drops,
//! rewards with an item choice, per-player markers and exact recovery.
mod support;

use std::sync::Arc;

use mmorpg_core::{
    Area, AreaId, CreatureBehaviour, CreatureFamily, CreatureId, CreatureLife, CreatureTemplate,
    CreatureTemplateId, EntityRef, ErrorCode, ItemId, ItemStack, MAX_QUEST_LOG, Npc, NpcId,
    NpcMarker, NpcRole, Quest, QuestEntry, QuestId, QuestMarker, QuestObjective, QuestRewards,
    ZoneAreas, ZoneCommand, ZoneContent, ZoneEvent, ZoneId, ZoneSimulation, greyhaven_vale,
};
use support::arena;

const BOAR: CreatureTemplateId = CreatureTemplateId::new(2);
const GIVER: NpcId = NpcId::new(1);
const FARAWAY: NpcId = NpcId::new(2);
const GUARD: NpcId = NpcId::new(3);
const FIELD: AreaId = AreaId::new(1);
const PELT: ItemId = ItemId::new(10);
const BOOTS: ItemId = ItemId::new(9);
const HOOD: ItemId = ItemId::new(6);
const BOARS: QuestId = QuestId::new(1);
const HIDES: QuestId = QuestId::new(2);
const ERRAND: QuestId = QuestId::new(3);
const SURVEY: QuestId = QuestId::new(4);
/// The first boar stands 2.5 m west of spawn slot 0, the second 2.4 m
/// south-west; the giver 3.5 m south-east.
const FIRST_BOAR: CreatureId = CreatureId::new(1);
const SECOND_BOAR: CreatureId = CreatureId::new(2);

/// A neutral boar that one swing kills.
fn boar() -> CreatureTemplate {
    CreatureTemplate {
        id: BOAR,
        name: "Test Boar".into(),
        family: CreatureFamily::Boar,
        behaviour: CreatureBehaviour::Neutral,
        health: 3,
        damage: [1, 1],
        ..arena::wolf(3, [1, 1])
    }
}

fn npc(id: NpcId, role: NpcRole, position: [i32; 2]) -> Npc {
    Npc {
        role,
        ..arena::npc(id.get(), position)
    }
}

fn quest(id: QuestId, ender: NpcId, objectives: Vec<QuestObjective>) -> Quest {
    Quest {
        id,
        name: format!("Quest {}", id.get()),
        text: "Do it.".into(),
        giver: GIVER,
        ender,
        prerequisite: None,
        objectives,
        rewards: QuestRewards::default(),
    }
}

/// The four test quests: two boars for 150 XP, 7 copper and boots or a
/// hood; then two pelts; talking to the faraway giver; exploring the field.
fn quests() -> Vec<Quest> {
    vec![
        Quest {
            rewards: QuestRewards {
                experience: 150,
                copper: 7,
                choices: vec![BOOTS, HOOD],
            },
            ..quest(
                BOARS,
                GIVER,
                vec![QuestObjective::Kill {
                    template: BOAR,
                    count: 2,
                }],
            )
        },
        Quest {
            prerequisite: Some(BOARS),
            rewards: QuestRewards {
                experience: 10,
                copper: 1,
                choices: Vec::new(),
            },
            ..quest(
                HIDES,
                GIVER,
                vec![QuestObjective::Collect {
                    item: PELT,
                    count: 2,
                    source: BOAR,
                }],
            )
        },
        quest(ERRAND, FARAWAY, vec![QuestObjective::Talk { npc: FARAWAY }]),
        quest(SURVEY, GIVER, vec![QuestObjective::Explore { area: FIELD }]),
    ]
}

fn base_content() -> ZoneContent {
    ZoneContent::new(
        arena::ground(),
        ZoneAreas::new(vec![
            Area::new(FIELD, "Far Field", [2_000, -4_000], [4_000, -2_000]).unwrap(),
        ])
        .unwrap(),
        vec![boar()],
        vec![
            arena::spawn(1, BOAR, [-250, 0]),
            arena::spawn(2, BOAR, [-170, -170]),
        ],
        vec![
            npc(GIVER, NpcRole::QuestGiver, [250, -250]),
            npc(FARAWAY, NpcRole::QuestGiver, [-1_500, -3_000]),
            npc(GUARD, NpcRole::Guard, [600, -600]),
        ],
        arena::GRAVEYARD,
    )
    .unwrap()
}

fn content() -> Arc<ZoneContent> {
    Arc::new(base_content().with_quests(1, quests()).unwrap())
}

struct Run {
    zone: ZoneSimulation,
    sequence: u32,
}

impl Run {
    fn new() -> Self {
        Self::with(content())
    }

    fn with(content: Arc<ZoneContent>) -> Self {
        let mut zone = ZoneSimulation::with_content(ZoneId::new(1), content).unwrap();
        zone.add_player(1).unwrap();
        Self { zone, sequence: 0 }
    }

    fn send(&mut self, command: ZoneCommand) {
        self.sequence += 1;
        self.zone.apply_command(1, self.sequence, command).unwrap();
    }

    /// Sends one command and advances one tick; returns its refusals and
    /// quest events.
    fn act(&mut self, command: ZoneCommand) -> Vec<ZoneEvent> {
        self.send(command);
        self.tick()
    }

    /// Advances one tick; returns its refusals and quest events (combat
    /// feedback is left out).
    fn tick(&mut self) -> Vec<ZoneEvent> {
        self.zone.advance_tick().unwrap();
        self.zone
            .snapshot_for_player(1)
            .unwrap()
            .events
            .into_iter()
            .filter(|event| {
                matches!(
                    event,
                    ZoneEvent::Error { .. }
                        | ZoneEvent::QuestProgress { .. }
                        | ZoneEvent::QuestCompleted { .. }
                )
            })
            .collect()
    }

    fn player(&self) -> mmorpg_core::CanonicalPlayerSnapshot {
        self.zone.snapshot().unwrap().players.remove(0)
    }

    /// Restores the zone with the player moved to `feet` (setup only).
    fn teleport(&mut self, feet: [i32; 2]) {
        let mut state = self.zone.snapshot().unwrap();
        state.players[0].position = [feet[0], 90, feet[1]];
        state.players[0].velocity = [0; 3];
        self.zone = ZoneSimulation::from_snapshot(state, Arc::clone(self.zone.content())).unwrap();
    }

    fn accept(&mut self, quest: QuestId) -> Vec<ZoneEvent> {
        self.act(ZoneCommand::AcceptQuest {
            npc: GIVER,
            quest: quest.get(),
        })
    }

    /// Kills one boar with auto-attack, standing at the origin.
    fn kill(&mut self, creature: CreatureId) -> Vec<ZoneEvent> {
        self.send(ZoneCommand::SelectTarget(Some(EntityRef::Creature(
            creature,
        ))));
        self.send(ZoneCommand::StartAttack);
        let mut events = Vec::new();
        for _ in 0..600 {
            events.extend(self.tick());
            let state = self.zone.snapshot().unwrap();
            let record = state
                .creatures
                .iter()
                .find(|record| record.creature_id == creature)
                .unwrap();
            if matches!(record.life, CreatureLife::Corpse { .. }) {
                return events;
            }
        }
        panic!("the boar survived");
    }

    fn markers(&mut self) -> Vec<NpcMarker> {
        // An unchanged sheet is re-sent every ten ticks.
        for _ in 0..10 {
            if let Some(sheet) = self.zone.snapshot_for_player(1).unwrap().quests {
                return sheet.markers;
            }
            self.zone.advance_tick().unwrap();
        }
        panic!("no quest sheet within ten ticks");
    }
}

fn error(code: ErrorCode, npc: Option<NpcId>) -> ZoneEvent {
    ZoneEvent::Error {
        code,
        target: npc.map(EntityRef::Npc),
    }
}

fn marker(npc: NpcId, marker: QuestMarker) -> NpcMarker {
    NpcMarker { npc, marker }
}

/// A named edit that makes the test quests invalid.
type Mutation = fn(&mut Vec<Quest>);

#[test]
fn content_validates_quests_and_binds_them_to_its_identity() {
    let base = base_content();
    let bound = base.clone().with_quests(1, quests()).unwrap();
    assert_ne!(bound.fingerprint(), base.fingerprint());
    assert_eq!(bound.rng_seed(), base.rng_seed(), "quests keep the seed");
    assert_eq!(bound.quest_revision(), 1);
    assert_eq!(
        bound
            .quests()
            .iter()
            .map(|quest| quest.id.get())
            .collect::<Vec<_>>(),
        [1, 2, 3, 4]
    );
    let mut reworded = quests();
    reworded[0].text = "Do it now.".into();
    assert_ne!(
        base.clone().with_quests(1, reworded).unwrap().fingerprint(),
        bound.fingerprint()
    );
    assert_ne!(
        base.clone().with_quests(2, quests()).unwrap().fingerprint(),
        bound.fingerprint()
    );

    let mutations: Vec<(&str, Mutation)> = vec![
        ("no quests", |quests| quests.clear()),
        ("id 0", |quests| quests[0].id = QuestId::new(0)),
        ("id 33", |quests| quests[0].id = QuestId::new(33)),
        ("duplicate", |quests| quests[1].id = BOARS),
        ("empty name", |quests| quests[0].name = " ".into()),
        ("long text", |quests| quests[0].text = "x".repeat(241)),
        ("guard giver", |quests| quests[0].giver = GUARD),
        ("unknown ender", |quests| quests[0].ender = NpcId::new(9)),
        ("later prerequisite", |quests| {
            quests[0].prerequisite = Some(HIDES);
        }),
        ("no objective", |quests| quests[0].objectives.clear()),
        ("unknown template", |quests| {
            quests[0].objectives[0] = QuestObjective::Kill {
                template: CreatureTemplateId::new(9),
                count: 1,
            };
        }),
        ("zero count", |quests| {
            quests[0].objectives[0] = QuestObjective::Kill {
                template: BOAR,
                count: 0,
            };
        }),
        ("equippable collect item", |quests| {
            quests[1].objectives[0] = QuestObjective::Collect {
                item: BOOTS,
                count: 1,
                source: BOAR,
            };
        }),
        ("repeated collect item", |quests| {
            quests[1].objectives.push(QuestObjective::Collect {
                item: PELT,
                count: 2,
                source: BOAR,
            });
        }),
        // Pelts stack to 10 in 16 bag slots.
        ("collect beyond a full bag", |quests| {
            quests[1].objectives[0] = QuestObjective::Collect {
                item: PELT,
                count: 161,
                source: BOAR,
            };
        }),
        ("unknown area", |quests| {
            quests[3].objectives[0] = QuestObjective::Explore {
                area: AreaId::new(2),
            };
        }),
        ("duplicate choice", |quests| {
            quests[0].rewards.choices = vec![BOOTS, BOOTS];
        }),
        ("unknown choice", |quests| {
            quests[0].rewards.choices = vec![ItemId::new(99)];
        }),
    ];
    for (name, mutate) in mutations {
        let mut invalid = quests();
        mutate(&mut invalid);
        assert!(base.clone().with_quests(1, invalid).is_err(), "{name}");
    }
    let mut full_bag = quests();
    full_bag[1].objectives[0] = QuestObjective::Collect {
        item: PELT,
        count: 160,
        source: BOAR,
    };
    assert!(
        base.clone().with_quests(1, full_bag).is_ok(),
        "a full bag of pelts"
    );
    assert!(base.with_quests(0, quests()).is_err(), "revision zero");

    let hosted = greyhaven_vale::content();
    assert_eq!(hosted.revision(), 9);
    assert_eq!(hosted.quests().len(), 9);
    assert_eq!(hosted.rng_seed(), greyhaven_vale::RNG_SEED);
}

#[test]
fn accepting_needs_the_giver_in_reach_and_an_available_quest() {
    let mut run = Run::new();
    assert_eq!(
        run.markers(),
        [marker(GIVER, QuestMarker::Available)],
        "the faraway ender of an unaccepted errand shows nothing"
    );
    run.teleport([2_000, 2_000]);
    let before = run.player();
    for (command, refusal) in [
        (
            ZoneCommand::AcceptQuest {
                npc: GIVER,
                quest: 1,
            },
            error(ErrorCode::OutOfRange, Some(GIVER)),
        ),
        (
            ZoneCommand::AcceptQuest {
                npc: GUARD,
                quest: 1,
            },
            error(ErrorCode::InvalidQuest, Some(GUARD)),
        ),
        (
            ZoneCommand::AcceptQuest {
                npc: GIVER,
                quest: 9,
            },
            error(ErrorCode::InvalidQuest, Some(GIVER)),
        ),
        (
            ZoneCommand::AcceptQuest {
                npc: GIVER,
                quest: HIDES.get(),
            },
            error(ErrorCode::InvalidQuest, Some(GIVER)),
        ),
        (
            ZoneCommand::AbandonQuest { quest: 1 },
            error(ErrorCode::InvalidQuest, None),
        ),
    ] {
        assert_eq!(run.act(command), [refusal], "{command:?}");
    }
    let after = run.player();
    assert_eq!(
        (after.quests, after.quests_changed_at),
        (before.quests, before.quests_changed_at)
    );

    run.teleport([0, 0]);
    assert!(run.accept(BOARS).is_empty());
    let player = run.player();
    assert_eq!(
        player.quests.entries,
        [QuestEntry {
            quest: BOARS,
            progress: [0; 3],
        }]
    );
    assert_eq!(player.quests_changed_at, run.zone.current_tick());
    assert!(
        run.zone.snapshot_for_player(1).unwrap().quests.is_some(),
        "the change tick sends the sheet"
    );
    assert_eq!(
        run.accept(BOARS),
        [error(ErrorCode::InvalidQuest, Some(GIVER))],
        "an active quest is not offered again"
    );
    // The giver still offers the survey and the errand.
    assert_eq!(run.markers(), [marker(GIVER, QuestMarker::Available)]);
    assert!(
        run.act(ZoneCommand::AbandonQuest { quest: BOARS.get() })
            .is_empty()
    );
    assert!(run.player().quests.entries.is_empty());
}

#[test]
fn the_log_holds_ten_quests_and_the_dead_cannot_take_more() {
    let many = (1..=11)
        .map(|id| {
            quest(
                QuestId::new(id),
                GIVER,
                vec![QuestObjective::Explore { area: FIELD }],
            )
        })
        .collect();
    let mut run = Run::with(Arc::new(base_content().with_quests(1, many).unwrap()));
    for id in 1..=10 {
        assert!(run.accept(QuestId::new(id)).is_empty(), "quest {id}");
    }
    assert_eq!(run.player().quests.entries.len(), MAX_QUEST_LOG);
    assert_eq!(
        run.accept(QuestId::new(11)),
        [error(ErrorCode::QuestLogFull, Some(GIVER))]
    );
    let mut state = run.zone.snapshot().unwrap();
    state.players[0].combat.health = 0;
    run.zone = ZoneSimulation::from_snapshot(state, Arc::clone(run.zone.content())).unwrap();
    assert_eq!(
        run.act(ZoneCommand::AbandonQuest { quest: 10 }),
        [],
        "the dead may abandon"
    );
    assert_eq!(
        run.accept(QuestId::new(11)),
        [error(ErrorCode::YouAreDead, None)]
    );
}

#[test]
fn kills_credit_the_tapper_and_the_turn_in_grants_the_chosen_reward() {
    let mut run = Run::new();
    // A kill before accepting counts for nothing.
    run.kill(FIRST_BOAR);
    assert!(run.accept(BOARS).is_empty());
    let events = run.kill(SECOND_BOAR);
    assert!(events.contains(&ZoneEvent::QuestProgress {
        quest: BOARS,
        objective: 0,
        count: 1,
    }));
    assert_eq!(run.player().quests.entries[0].progress, [1, 0, 0]);
    // The errand and the survey are still on offer: `!` outranks grey `?`.
    assert_eq!(run.markers(), [marker(GIVER, QuestMarker::Available)]);
    let incomplete = run.player();
    assert_eq!(
        run.act(ZoneCommand::CompleteQuest {
            npc: GIVER,
            quest: BOARS.get(),
            choice: 0,
        }),
        [error(ErrorCode::QuestIncomplete, Some(GIVER))]
    );
    assert_eq!(run.player().quests, incomplete.quests);

    // The first boar respawns after a minute.
    for _ in 0..1_800 {
        run.tick();
    }
    run.kill(FIRST_BOAR);
    assert_eq!(run.player().quests.entries[0].progress, [2, 0, 0]);
    assert_eq!(run.markers(), [marker(GIVER, QuestMarker::Complete)]);

    for (choice, ender) in [(2, GIVER), (0, FARAWAY)] {
        assert_eq!(
            run.act(ZoneCommand::CompleteQuest {
                npc: ender,
                quest: BOARS.get(),
                choice,
            }),
            [error(ErrorCode::InvalidQuest, Some(ender))]
        );
    }
    let before = run.player();
    let events = run.act(ZoneCommand::CompleteQuest {
        npc: GIVER,
        quest: BOARS.get(),
        choice: 1,
    });
    assert!(events.contains(&ZoneEvent::QuestCompleted { quest: BOARS }));
    let after = run.player();
    assert!(after.quests.entries.is_empty());
    assert_eq!(after.quests.completed, 0b1);
    assert_eq!(after.copper, before.copper + 7);
    assert_eq!(after.inventory.count(HOOD), 1, "the second choice");
    assert_eq!(after.inventory.count(BOOTS), 0);
    assert_eq!(after.inventory_revision, before.inventory_revision + 1);
    // Three boar kills (50 + 50 + 40 XP) reached level 2 with 40; the
    // quest's 150 stay below level 2's 200.
    assert_eq!(
        (after.combat.level, after.combat.experience),
        (2, before.combat.experience + 150)
    );
    assert_eq!(
        run.accept(BOARS),
        [error(ErrorCode::InvalidQuest, Some(GIVER))],
        "a turned-in quest is not offered again"
    );
    // Turning in unlocked the hides.
    assert_eq!(run.markers(), [marker(GIVER, QuestMarker::Available)]);
}

#[test]
fn quest_items_drop_only_while_needed_and_the_turn_in_consumes_them() {
    let mut run = Run::new();
    let mut state = run.zone.snapshot().unwrap();
    state.players[0].quests.completed = 0b1;
    run.zone = ZoneSimulation::from_snapshot(state, Arc::clone(run.zone.content())).unwrap();
    run.kill(FIRST_BOAR);
    let corpse = |run: &Run, creature| {
        run.zone
            .snapshot()
            .unwrap()
            .creatures
            .into_iter()
            .find(|record| record.creature_id == creature)
            .unwrap()
    };
    assert_eq!(
        corpse(&run, FIRST_BOAR)
            .loot
            .and_then(|loot| loot.quest_item),
        None,
        "no pelt without the quest"
    );
    assert!(run.accept(HIDES).is_empty());
    run.kill(SECOND_BOAR);
    let rewards = corpse(&run, SECOND_BOAR).loot.unwrap();
    assert_eq!(rewards.quest_item, Some(ItemStack::new(PELT, 1).unwrap()));
    assert_eq!((rewards.money, rewards.item), (0, None), "no loot table");
    let view = run.zone.snapshot_for_player(1).unwrap().loot.unwrap();
    let events = run.act(ZoneCommand::Loot(view.claim));
    assert!(events.contains(&ZoneEvent::QuestProgress {
        quest: HIDES,
        objective: 0,
        count: 1,
    }));
    assert_eq!(run.player().inventory.count(PELT), 1);
    let sheet = run.zone.snapshot_for_player(1).unwrap().quests.unwrap();
    assert_eq!(
        sheet.entries[0].progress,
        [1, 0, 0],
        "collect counts the bag"
    );
    assert_eq!(run.player().quests.entries[0].progress, [0; 3]);

    // A second pelt, granted directly, completes it; a third kill drops none.
    let mut state = run.zone.snapshot().unwrap();
    let mut bag = state.players[0].inventory.clone();
    bag.insert(PELT, 1).unwrap();
    state.players[0].inventory = bag;
    run.zone = ZoneSimulation::from_snapshot(state, Arc::clone(run.zone.content())).unwrap();
    for _ in 0..1_800 {
        run.tick();
    }
    run.kill(FIRST_BOAR);
    assert_eq!(
        corpse(&run, FIRST_BOAR)
            .loot
            .and_then(|loot| loot.quest_item),
        None
    );
    assert_eq!(run.markers(), [marker(GIVER, QuestMarker::Complete)]);
    let before = run.player();
    assert!(
        run.act(ZoneCommand::CompleteQuest {
            npc: GIVER,
            quest: HIDES.get(),
            choice: 0,
        })
        .contains(&ZoneEvent::QuestCompleted { quest: HIDES })
    );
    let after = run.player();
    assert_eq!(after.inventory.count(PELT), 0);
    assert_eq!(after.copper, before.copper + 1);
    assert_eq!(after.quests.completed, 0b11);
}

#[test]
fn talking_and_exploring_complete_on_arrival() {
    let mut run = Run::new();
    assert!(run.accept(ERRAND).is_empty());
    assert!(run.accept(SURVEY).is_empty());
    // `!` for the boars outranks the survey's grey `?`.
    assert_eq!(
        run.markers(),
        [
            marker(GIVER, QuestMarker::Available),
            marker(FARAWAY, QuestMarker::InProgress),
        ]
    );
    // Within 5 m of the faraway ender's feet.
    run.teleport([-1_500, -2_550]);
    let events = run.tick();
    assert_eq!(
        events,
        [ZoneEvent::QuestProgress {
            quest: ERRAND,
            objective: 0,
            count: 1,
        }]
    );
    assert_eq!(
        run.markers(),
        [
            marker(GIVER, QuestMarker::Available),
            marker(FARAWAY, QuestMarker::Complete),
        ]
    );
    assert!(
        run.act(ZoneCommand::CompleteQuest {
            npc: FARAWAY,
            quest: ERRAND.get(),
            choice: 0,
        })
        .contains(&ZoneEvent::QuestCompleted { quest: ERRAND })
    );
    run.teleport([3_000, -3_000]);
    assert_eq!(
        run.tick(),
        [ZoneEvent::QuestProgress {
            quest: SURVEY,
            objective: 0,
            count: 1,
        }]
    );
    assert!(run.tick().is_empty(), "progress is reported once");
    assert_eq!(run.player().quests.entries[0].progress, [1, 0, 0]);
}

#[test]
fn recovery_continues_quests_exactly_and_refuses_inconsistent_logs() {
    let mut run = Run::new();
    run.accept(BOARS);
    run.send(ZoneCommand::AcceptQuest {
        npc: GIVER,
        quest: SURVEY.get(),
    });
    run.send(ZoneCommand::SelectTarget(Some(EntityRef::Creature(
        FIRST_BOAR,
    ))));
    run.send(ZoneCommand::StartAttack);
    let checkpoint = run.zone.snapshot().unwrap();
    let mut recovered =
        ZoneSimulation::from_snapshot(checkpoint.clone(), Arc::clone(run.zone.content())).unwrap();
    for _ in 0..200 {
        run.zone.advance_tick().unwrap();
        recovered.advance_tick().unwrap();
        assert_eq!(recovered.snapshot().unwrap(), run.zone.snapshot().unwrap());
        assert_eq!(
            recovered.snapshot_for_player(1).unwrap(),
            run.zone.snapshot_for_player(1).unwrap()
        );
    }
    assert_eq!(run.player().quests.entries.len(), 2);

    let content = Arc::clone(run.zone.content());
    let refuse = |mutate: &dyn Fn(&mut mmorpg_core::CanonicalPlayerSnapshot)| {
        let mut state = checkpoint.clone();
        mutate(&mut state.players[0]);
        ZoneSimulation::from_snapshot(state, Arc::clone(&content)).is_err()
    };
    assert!(refuse(&|player| player
        .quests
        .entries
        .push(player.quests.entries[0])));
    assert!(refuse(&|player| player.quests.entries[0].progress[0] = 3));
    assert!(refuse(&|player| player.quests.entries[0].progress[1] = 1));
    assert!(refuse(&|player| player.quests.completed = 0b1));
    assert!(refuse(&|player| player.quests.completed = 1 << 9));
    assert!(refuse(&|player| player.quests.entries[0].quest = HIDES));
    assert!(refuse(&|player| player.quests_changed_at = u64::MAX));
    assert!(refuse(&|player| {
        player.quests.entries = (1..=11)
            .map(|id| QuestEntry {
                quest: QuestId::new(id),
                progress: [0; 3],
            })
            .collect();
    }));
    // Content without quests holds no log.
    let mut state = checkpoint.clone();
    let plain = Arc::new(base_content());
    state.content_fingerprint = plain.fingerprint();
    assert!(ZoneSimulation::from_snapshot(state, plain).is_err());
}

#[test]
fn the_hosted_chain_starts_at_the_marshal() {
    let mut zone = ZoneSimulation::with_content(ZoneId::new(1), greyhaven_vale::content()).unwrap();
    zone.add_player(1).unwrap();
    let sheet = zone.snapshot_for_player(1).unwrap().quests.unwrap();
    assert_eq!(
        sheet.markers,
        [marker(
            greyhaven_vale::quests::MARSHAL,
            QuestMarker::Available
        )]
    );
    assert!(sheet.entries.is_empty() && sheet.completed == 0);
}
