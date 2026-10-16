//! Quest acceptance (#25): on the hosted Greyhaven Vale content, a new
//! Warden completes the first quests of the Greyhaven chain through
//! ordinary commands only. It walks from the spawn plaza to Marshal Elden
//! Greywatch, hunts Timber Wolves in the Wolfrun Woods for *Trouble in the
//! Woods*, collects their quest-only pelts for Tanner Hilda Brook, then
//! follows the Millbrook Road to Farmer Osric Mill for *The Missing
//! Farmhand*. Every outcome comes from the real content, AI and zone RNG.
use mmorpg_core::ability::ids;
use mmorpg_core::greyhaven_vale::quests::{
    FARMER, MARAUDERS_IN_THE_FIELDS, MARSHAL, PELTS_FOR_THE_TANNER, TANNER, THE_MISSING_FARMHAND,
    TROUBLE_IN_THE_WOODS, WOLF_PELT,
};
use mmorpg_core::{
    EntityKind, NpcId, NpcMarker, QuestId, QuestMarker, QuestSheet, ZoneCommand, ZoneEvent, ZoneId,
    ZoneSimulation, ZoneSnapshot, greyhaven_vale,
};

/// Generous bound on the whole run (about 28 minutes of game time).
const MAX_TICKS: u64 = 30 * 60 * 28;
const TIMBER_WOLF: u16 = 1;
/// The plaza's west edge, where every trip starts and ends.
const HUB: [i32; 2] = [-1_800, 2_000];
/// Each trip ends within quest reach of the NPC, arrival slack included.
const TO_MARSHAL: &[[i32; 2]] = &[[-2_200, 1_150]];
const TO_TANNER: &[[i32; 2]] = &[[-2_400, 2_850]];
/// The Wolfrun Trail through the west gate to the den clearing.
const TO_WOODS: &[[i32; 2]] = &[
    [-3_200, 2_000],
    [-4_400, 2_000],
    [-5_600, 900],
    [-6_600, 300],
    [-7_600, 0],
];
/// The Millbrook Road through the east gate to the farmhouse.
const TO_FARMER: &[[i32; 2]] = &[
    [1_600, 2_000],
    [3_200, 2_000],
    [4_400, 2_000],
    [5_400, 2_800],
    [6_400, 3_300],
    [6_200, 3_850],
];
/// Released spirits rise beside the graveyard, north of the hub.
const FROM_GRAVEYARD: &[[i32; 2]] = &[[-1_800, 4_150], HUB];

struct Script {
    zone: ZoneSimulation,
    sequence: u32,
    /// The held movement last sent.
    held: (i8, i8, u16),
    ticks: u64,
    view: ZoneSnapshot,
    sheet: QuestSheet,
    /// Positions of the last ticks spent walking, to notice a blocked path.
    trail: Vec<[i32; 3]>,
    sidestep: u16,
    sidestep_right: bool,
    deaths: u32,
    events: Vec<ZoneEvent>,
}

impl Script {
    fn new() -> Self {
        let mut zone =
            ZoneSimulation::with_content(ZoneId::new(1), greyhaven_vale::content()).unwrap();
        zone.add_player(1).unwrap();
        let view = zone.snapshot_for_player(1).unwrap();
        let sheet = view.quests.clone().unwrap();
        let mut script = Self {
            zone,
            sequence: 0,
            held: (0, 0, 0),
            ticks: 0,
            view,
            sheet,
            trail: Vec::new(),
            sidestep: 0,
            sidestep_right: true,
            deaths: 0,
            events: Vec::new(),
        };
        script.send(ZoneCommand::ChooseClass { class: 0, sex: 1 });
        script.tick();
        script
    }

    fn send(&mut self, command: ZoneCommand) {
        self.sequence += 1;
        self.zone.apply_command(1, self.sequence, command).unwrap();
    }

    fn tick(&mut self) {
        self.zone.advance_tick().unwrap();
        self.ticks += 1;
        assert!(
            self.ticks < MAX_TICKS,
            "the chain took over {MAX_TICKS} ticks"
        );
        self.view = self.zone.snapshot_for_player(1).unwrap();
        if let Some(sheet) = &self.view.quests {
            self.sheet = sheet.clone();
        }
        self.events.extend(self.view.events.iter().copied());
    }

    fn me(&self) -> [i32; 3] {
        self.view.entities[0].position
    }

    fn hold(&mut self, forward: i8, strafe: i8, facing: u16) {
        let changed = (forward, strafe) != (self.held.0, self.held.1)
            || (forward != 0 || strafe != 0) && self.held.2.abs_diff(facing) > 512;
        if changed {
            self.held = (forward, strafe, facing);
            self.send(ZoneCommand::Move {
                forward,
                strafe,
                facing,
            });
        }
    }

    fn stand(&mut self) {
        self.trail.clear();
        self.hold(0, 0, self.held.2);
    }

    /// Steers toward `goal` and reports arrival within `reach` units. A
    /// walk that stops making progress sidesteps a trunk or wall.
    fn steer(&mut self, goal: [i32; 3], reach: i32) -> bool {
        let me = self.me();
        let [dx, dz] = [f64::from(goal[0] - me[0]), f64::from(goal[2] - me[2])];
        if dx.hypot(dz) <= f64::from(reach) {
            self.stand();
            return true;
        }
        let facing = ((dx.atan2(dz) / std::f64::consts::TAU).rem_euclid(1.0) * 65_536.0) as u16;
        if self.sidestep > 0 {
            self.sidestep -= 1;
            let strafe = if self.sidestep_right { 1 } else { -1 };
            self.hold(0, strafe, facing);
            return false;
        }
        self.trail.push(me);
        if self.trail.len() > 12 {
            self.trail.remove(0);
            let start = self.trail[0];
            let moved = f64::from(me[0] - start[0]).hypot(f64::from(me[2] - start[2]));
            if moved < 60.0 {
                self.trail.clear();
                self.sidestep = 18;
                self.sidestep_right = !self.sidestep_right;
            }
        }
        self.hold(1, 0, facing);
        false
    }

    fn rage(&self) -> u16 {
        self.view.viewer.resource.map_or(0, |rage| rage.value)
    }

    /// One tick of handling whatever needs handling before travel: death,
    /// an attacker, a fight in progress, an owned corpse with loot, or
    /// missing health. Returns `false` when the Warden is free to go on.
    fn handle(&mut self) -> bool {
        if self.view.viewer.dead {
            self.deaths += 1;
            self.send(ZoneCommand::ReleaseSpirit);
            self.held = (0, 0, self.held.2);
            self.tick();
            self.walk_path(FROM_GRAVEYARD);
            return true;
        }
        if let Some(loot) = self.view.loot {
            self.send(ZoneCommand::Loot(loot.claim));
            self.send(ZoneCommand::SelectTarget(None));
            self.tick();
            return true;
        }
        let target = self
            .view
            .entities
            .iter()
            .find(|unit| Some(unit.entity()) == self.view.viewer.target)
            .cloned();
        if let Some(unit) = target {
            if !unit.flags.dead {
                if self.steer(unit.position, 180) {
                    if !self.view.viewer.auto_attacking {
                        self.send(ZoneCommand::StartAttack);
                    } else if self.rage() >= 15 {
                        self.send(ZoneCommand::UseAbility {
                            ability: ids::HEROIC_STRIKE.get(),
                            target: None,
                        });
                    }
                }
                self.tick();
                return true;
            }
            if unit.flags.lootable {
                self.steer(unit.position, 150);
                self.tick();
                return true;
            }
            self.send(ZoneCommand::SelectTarget(None));
        }
        let attacker = self
            .view
            .entities
            .iter()
            .filter(|unit| unit.flags.targets_viewer && !unit.flags.dead && unit.flags.attackable)
            .min_by_key(|unit| unit.id)
            .map(|unit| unit.entity());
        if let Some(attacker) = attacker {
            self.send(ZoneCommand::SelectTarget(Some(attacker)));
            self.tick();
            return true;
        }
        if self.view.viewer.health < self.view.viewer.max_health {
            self.stand();
            self.tick();
            return true;
        }
        false
    }

    /// Walks the waypoints, fighting off attackers on the way. A death
    /// restarts the walk from the hub.
    fn walk_path(&mut self, path: &[[i32; 2]]) {
        let deaths = self.deaths;
        let mut next = 0;
        while next < path.len() {
            if self.handle() {
                if self.deaths != deaths {
                    // The spirit is back at the hub; the trip starts over.
                    return self.walk_path(path_from_hub(path));
                }
                continue;
            }
            let [x, z] = path[next];
            if self.steer([x, 0, z], 120) {
                next += 1;
            }
            self.tick();
        }
        self.stand();
    }

    fn progress(&self, quest: QuestId) -> Option<[u8; 3]> {
        self.sheet
            .entries
            .iter()
            .find(|entry| entry.quest == quest)
            .map(|entry| entry.progress)
    }

    fn marker(&self, npc: NpcId) -> Option<QuestMarker> {
        self.sheet
            .markers
            .iter()
            .find(|marker| marker.npc == npc)
            .map(|marker| marker.marker)
    }

    /// Sends one quest command, then waits for the sheet that reflects it.
    fn quest_command(&mut self, command: ZoneCommand) {
        self.stand();
        self.events.clear();
        self.send(command);
        self.tick();
        assert!(
            self.view.quests.is_some(),
            "a quest change sends the sheet: {:?}",
            self.events
        );
        assert!(
            !self
                .events
                .iter()
                .any(|event| matches!(event, ZoneEvent::Error { .. })),
            "{command:?} was refused: {:?}",
            self.events
        );
    }

    /// Hunts Timber Wolves near the den clearing until `done`.
    fn hunt(&mut self, done: impl Fn(&Self) -> bool) {
        let deaths = self.deaths;
        while !done(self) {
            if self.handle() {
                if self.deaths != deaths {
                    self.walk_path(TO_WOODS);
                    return self.hunt(done);
                }
                continue;
            }
            let me = self.me();
            let prey = self
                .view
                .entities
                .iter()
                .filter(|unit| {
                    unit.kind == EntityKind::Creature
                        && unit.appearance == TIMBER_WOLF
                        && !unit.flags.dead
                        && !unit.flags.tapped_by_other
                })
                .min_by_key(|unit| {
                    let [dx, dz] = [unit.position[0] - me[0], unit.position[2] - me[2]];
                    (i64::from(dx).pow(2) + i64::from(dz).pow(2), unit.id)
                })
                .map(|unit| unit.entity());
            match prey {
                Some(wolf) => self.send(ZoneCommand::SelectTarget(Some(wolf))),
                // Wait in the clearing for a respawn.
                None => {
                    self.steer([-8_000, 0, 0], 300);
                }
            }
            self.tick();
        }
        self.stand();
    }
}

/// The part of a trip after a death: from the hub when the trip passed it.
fn path_from_hub(path: &[[i32; 2]]) -> &[[i32; 2]] {
    match path.iter().position(|&point| point == HUB) {
        Some(index) => &path[index + 1..],
        None => path,
    }
}

fn marker(npc: NpcId, marker: QuestMarker) -> NpcMarker {
    NpcMarker { npc, marker }
}

#[test]
fn a_new_warden_completes_the_first_quests_of_the_greyhaven_chain() {
    let mut script = Script::new();
    assert_eq!(
        script.sheet.markers,
        [marker(MARSHAL, QuestMarker::Available)]
    );

    // Trouble in the Woods: six Timber Wolves for the Marshal.
    script.walk_path(TO_MARSHAL);
    script.quest_command(ZoneCommand::AcceptQuest {
        npc: MARSHAL,
        quest: TROUBLE_IN_THE_WOODS.get(),
    });
    assert_eq!(script.marker(MARSHAL), Some(QuestMarker::InProgress));
    script.walk_path(&[HUB]);
    script.walk_path(TO_WOODS);
    script.hunt(|script| script.progress(TROUBLE_IN_THE_WOODS) == Some([6, 0, 0]));
    assert_eq!(script.marker(MARSHAL), Some(QuestMarker::Complete));
    script.walk_path(&[TO_WOODS[3], TO_WOODS[2], TO_WOODS[1], TO_WOODS[0], HUB]);
    script.walk_path(TO_MARSHAL);
    let copper = script.view.viewer.copper;
    script.quest_command(ZoneCommand::CompleteQuest {
        npc: MARSHAL,
        quest: TROUBLE_IN_THE_WOODS.get(),
        choice: 0,
    });
    assert!(script.events.contains(&ZoneEvent::QuestCompleted {
        quest: TROUBLE_IN_THE_WOODS
    }));
    assert_eq!(script.view.viewer.copper, copper + 10);
    assert_eq!(script.sheet.completed, 0b1);
    assert_eq!(script.marker(TANNER), Some(QuestMarker::Available));

    // Pelts for the Tanner: five quest-only pelts from Timber Wolves.
    script.walk_path(&[HUB]);
    script.walk_path(TO_TANNER);
    script.quest_command(ZoneCommand::AcceptQuest {
        npc: TANNER,
        quest: PELTS_FOR_THE_TANNER.get(),
    });
    script.walk_path(&[HUB]);
    script.walk_path(TO_WOODS);
    script.hunt(|script| script.progress(PELTS_FOR_THE_TANNER) == Some([5, 0, 0]));
    let bag = script.zone.snapshot().unwrap().players[0].inventory.clone();
    assert_eq!(bag.count(WOLF_PELT), 5, "pelts drop only while needed");
    script.walk_path(&[TO_WOODS[3], TO_WOODS[2], TO_WOODS[1], TO_WOODS[0], HUB]);
    script.walk_path(TO_TANNER);
    script.quest_command(ZoneCommand::CompleteQuest {
        npc: TANNER,
        quest: PELTS_FOR_THE_TANNER.get(),
        choice: 0,
    });
    let player = script.zone.snapshot().unwrap().players.remove(0);
    assert_eq!(
        player.inventory.count(WOLF_PELT),
        0,
        "the turn-in takes them"
    );
    assert_eq!(
        player.inventory.count(mmorpg_core::ItemId::new(9)),
        1,
        "the Worn Boots were chosen"
    );

    // The Missing Farmhand: talk to Farmer Osric Mill at Millbrook.
    script.quest_command(ZoneCommand::AcceptQuest {
        npc: TANNER,
        quest: THE_MISSING_FARMHAND.get(),
    });
    assert_eq!(script.marker(FARMER), Some(QuestMarker::InProgress));
    script.walk_path(&[HUB]);
    script.walk_path(TO_FARMER);
    for _ in 0..10 {
        script.tick();
    }
    assert_eq!(script.progress(THE_MISSING_FARMHAND), Some([1, 0, 0]));
    assert_eq!(script.marker(FARMER), Some(QuestMarker::Complete));
    script.quest_command(ZoneCommand::CompleteQuest {
        npc: FARMER,
        quest: THE_MISSING_FARMHAND.get(),
        choice: 0,
    });
    assert_eq!(script.sheet.completed, 0b111);
    assert!(script.sheet.entries.is_empty());
    // Osric now offers Marauders in the Fields.
    assert_eq!(script.marker(FARMER), Some(QuestMarker::Available));
    assert!(
        script
            .zone
            .content()
            .quest(MARAUDERS_IN_THE_FIELDS)
            .is_some_and(|quest| quest.giver == FARMER)
    );
    assert!(script.view.viewer.level >= 3, "kills and quests level up");
    eprintln!(
        "three quests in {} ticks at level {} with {} deaths",
        script.ticks, script.view.viewer.level, script.deaths
    );
}
