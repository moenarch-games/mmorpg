//! The deterministic drivers the physics backend A/B test and timing example run
//! (physics-engine #330): existing headless bot scenarios replayed command by command
//! against the real zone simulation, and the Greyhaven combat workload script. A driver
//! builds its zone on whichever backend `physics::using_backend` selects on this thread, so
//! both backends see the same initial state, inputs and tick count.
//!
//! Bot scenarios are the `mmorpg-scenarios` TOML files. The bot runner admits each bot
//! through `game-server`, whose sessions number players from 1 in admission order, and
//! submits each command with the bot's next sequence; this replay applies the same commands
//! directly to the zone. Only the actions the replayed scenarios use are supported.

use std::error::Error;

use mmorpg_core::{CreatureId, EntityRef, NpcId, PlayerId, ZoneCommand, ZoneId, ZoneSimulation};

use crate::greyhaven_workload::{self, Fixture};

/// A unit a bot step names.
#[derive(Clone, Debug)]
enum Unit {
    None,
    Bot(usize),
    Creature(u32),
    Npc(u32),
}

#[derive(Clone, Debug)]
enum Action {
    Join,
    Move {
        forward: i8,
        strafe: i8,
        facing: u16,
    },
    Jump,
    SelectTarget(Unit),
    StartAttack,
    StopAttack,
    ReleaseSpirit,
}

#[derive(Clone, Debug)]
struct Step {
    tick: u64,
    bot: usize,
    action: Action,
}

/// A scenario `position` expectation: the named player's rounded position at `tick`.
#[derive(Clone, Debug)]
pub struct PositionExpectation {
    pub tick: u64,
    pub bot: usize,
    pub position: [i32; 3],
}

/// A parsed bot scenario.
#[derive(Clone, Debug)]
pub struct BotScenario {
    pub name: String,
    pub zone: u32,
    pub ticks: u64,
    pub bots: Vec<String>,
    steps: Vec<Step>,
    pub positions: Vec<PositionExpectation>,
}

fn field<'a>(table: &'a toml::Table, key: &str) -> Result<&'a toml::Value, String> {
    table.get(key).ok_or_else(|| format!("missing {key}"))
}

fn integer<T: TryFrom<i64>>(table: &toml::Table, key: &str) -> Result<T, String> {
    let value = field(table, key)?
        .as_integer()
        .ok_or_else(|| format!("{key} is not an integer"))?;
    T::try_from(value).map_err(|_| format!("{key} is out of range"))
}

fn text<'a>(table: &'a toml::Table, key: &str) -> Result<&'a str, String> {
    field(table, key)?
        .as_str()
        .ok_or_else(|| format!("{key} is not a string"))
}

fn tables<'a>(table: &'a toml::Table, key: &str) -> Vec<&'a toml::Table> {
    table
        .get(key)
        .and_then(toml::Value::as_array)
        .map(|items| items.iter().filter_map(toml::Value::as_table).collect())
        .unwrap_or_default()
}

impl BotScenario {
    /// Parses a bot scenario file; fails on any action this replay does not support.
    pub fn parse(source: &str) -> Result<Self, String> {
        let table: toml::Table = source.parse().map_err(|error| format!("{error}"))?;
        let bots: Vec<String> = tables(&table, "bots")
            .iter()
            .map(|bot| text(bot, "name").map(str::to_owned))
            .collect::<Result<_, _>>()?;
        let bot = |name: &str| {
            bots.iter()
                .position(|bot| bot == name)
                .ok_or_else(|| format!("unknown bot {name}"))
        };
        let unit = |spec: &str| -> Result<Unit, String> {
            if spec == "none" {
                return Ok(Unit::None);
            }
            let (kind, value) = spec
                .split_once(':')
                .ok_or_else(|| format!("bad unit {spec}"))?;
            let id = || value.parse::<u32>().map_err(|_| format!("bad unit {spec}"));
            Ok(match kind {
                "bot" => Unit::Bot(bot(value)?),
                "creature" => Unit::Creature(id()?),
                "npc" => Unit::Npc(id()?),
                _ => return Err(format!("bad unit {spec}")),
            })
        };
        let mut steps = Vec::new();
        for step in tables(&table, "steps") {
            if step.contains_key("seq") || step.contains_key("connection_epoch") {
                return Err("explicit sequences are not replayed".into());
            }
            let action = match text(step, "action")? {
                "join" => Action::Join,
                "move" => Action::Move {
                    forward: integer(step, "forward")?,
                    strafe: integer(step, "strafe")?,
                    facing: integer(step, "facing")?,
                },
                "jump" => Action::Jump,
                "select_target" => Action::SelectTarget(unit(text(step, "entity")?)?),
                "start_attack" => Action::StartAttack,
                "stop_attack" => Action::StopAttack,
                "release_spirit" => Action::ReleaseSpirit,
                other => return Err(format!("action {other} is not replayed")),
            };
            steps.push(Step {
                tick: integer(step, "tick")?,
                bot: bot(text(step, "bot")?)?,
                action,
            });
        }
        let mut positions = Vec::new();
        for expectation in tables(&table, "expect") {
            if text(expectation, "kind")? != "position" {
                continue;
            }
            let player = expectation
                .get("target")
                .and_then(toml::Value::as_str)
                .map_or_else(|| text(expectation, "bot"), Ok)?;
            let position = field(expectation, "position")?
                .as_array()
                .and_then(|axes| {
                    let axes: Vec<_> = axes
                        .iter()
                        .filter_map(|axis| i32::try_from(axis.as_integer()?).ok())
                        .collect();
                    <[i32; 3]>::try_from(axes).ok()
                })
                .ok_or("bad expected position")?;
            positions.push(PositionExpectation {
                tick: integer(expectation, "tick")?,
                bot: bot(player)?,
                position,
            });
        }
        Ok(Self {
            name: text(&table, "name")?.to_owned(),
            zone: integer(&table, "zone")?,
            ticks: integer(&table, "ticks")?,
            bots,
            steps,
            positions,
        })
    }
}

/// One deterministic driver.
#[derive(Clone, Debug)]
pub enum Workload {
    Bots(BotScenario),
    Greyhaven(Fixture),
    /// The hill ogre (#297): body-blocking, shoving and stomping in Southmeadow Downs.
    OgreBrawl,
    /// The mill hoist (#56): a player rides the moving platform up and walks off at the top.
    /// Unsupported on physics-engine (no moving support).
    MillHoist,
    /// Charges through Wolfrun Woods (#57): four level-6 Wardens charge at wolves among the
    /// trunks; a charge needs a path as wide as the body (`World::cast_shape`).
    ChargePath,
    /// The mill dock's ramps (#59): one player walks up the cart ramp onto the dock and back
    /// down and across it, another walks into the steep chute.
    MillRamps,
}

/// The charge workload's length.
pub const CHARGE_PATH_TICKS: usize = 240;

/// The Wardens by ID and feet XZ, on Wolfrun Woods trunk-grid corners near wolves.
const CHARGE_PATH_PLAYERS: [(u32, [f32; 2]); 4] = [
    (1, [-7_550.0, -2_700.0]),
    (2, [-6_650.0, -2_250.0]),
    (3, [-8_900.0, -2_700.0]),
    (4, [-7_100.0, -900.0]),
];

/// The charge workload's commands: each Warden chooses its class, targets a wolf and charges.
fn charge_path_commands(tick: usize) -> Vec<(u32, ZoneCommand)> {
    // Wolves 102, 103, 101 and 107 (Greyhaven spawns).
    let wolves = [102, 103, 101, 107];
    (1..=4)
        .flat_map(|player: u32| {
            let wolf = wolves[usize::try_from(player - 1).expect("four players")];
            match tick {
                0 => vec![(player, ZoneCommand::ChooseClass { class: 0, sex: 0 })],
                1 => vec![(
                    player,
                    ZoneCommand::SelectTarget(Some(EntityRef::Creature(CreatureId::new(wolf)))),
                )],
                2 => vec![(
                    player,
                    ZoneCommand::UseAbility {
                        ability: mmorpg_core::ability::ids::CHARGE.get(),
                        target: None,
                    },
                )],
                _ => Vec::new(),
            }
        })
        .collect()
}

/// The ramp walk's length.
pub const MILL_RAMPS_TICKS: usize = 240;

/// The ramp walk's players by ID and feet XZ.
const MILL_RAMPS_PLAYERS: [(u32, [f32; 2]); 2] = [(1, [7_300.0, 1_950.0]), (2, [8_250.0, 1_500.0])];

/// The ramp walk's commands at `tick`.
fn mill_ramps_commands(tick: usize) -> Vec<(u32, ZoneCommand)> {
    let walk = |facing| ZoneCommand::Move {
        forward: 1,
        strafe: 0,
        facing,
    };
    let (east, west, plus_z, minus_z) = (16_384, 49_152, 0, 32_768);
    match tick {
        0 => vec![(1, walk(east)), (2, walk(plus_z))],
        44 => vec![(1, walk(west))],
        // Back on the ramp, across its slope.
        60 => vec![(1, walk(plus_z))],
        64 => vec![(1, walk(minus_z))],
        72 => vec![(1, walk(west))],
        120 => vec![(1, walk(east)), (2, walk(minus_z))],
        150 => vec![
            (
                1,
                ZoneCommand::Move {
                    forward: 0,
                    strafe: 0,
                    facing: east,
                },
            ),
            (
                2,
                ZoneCommand::Move {
                    forward: 0,
                    strafe: 0,
                    facing: plus_z,
                },
            ),
        ],
        _ => Vec::new(),
    }
}

/// The mill hoist ride: one full cycle and a little more.
pub const MILL_HOIST_TICKS: usize = 640;

/// The hoist ride's commands: player 1 stands on the resting hoist from the start and walks
/// north onto the windmill's gallery once the hoist is up.
fn mill_hoist_commands(tick: usize) -> Vec<(u32, ZoneCommand)> {
    let top = mmorpg_core::greyhaven_vale::MILL_HOIST;
    let up = usize::try_from(top.dwell + top.travel_ticks()).expect("ticks fit");
    let walk = |forward| ZoneCommand::Move {
        forward,
        strafe: 0,
        facing: 0,
    };
    if tick == up + 10 {
        vec![(1, walk(1))]
    } else if tick == up + 30 {
        vec![(1, walk(0))]
    } else {
        Vec::new()
    }
}

/// The hoist ride's initial zone: Greyhaven with player 1 on the resting hoist.
fn mill_hoist_zone() -> Result<ZoneSimulation, Box<dyn Error>> {
    let content = mmorpg_core::greyhaven_vale::content();
    let mut zone = ZoneSimulation::with_content(ZoneId::new(1), std::sync::Arc::clone(&content))?;
    zone.add_player(1)?;
    let mut state = zone.snapshot()?;
    let [x, _, z] = mmorpg_core::greyhaven_vale::MILL_HOIST.low;
    #[allow(clippy::cast_precision_loss)]
    let feet = [x as f32, 90.0, z as f32];
    state.players[0].position = feet;
    Ok(ZoneSimulation::from_snapshot(state, content)?)
}

/// The ogre brawl's script length.
pub const OGRE_BRAWL_TICKS: usize = 360;
/// The hill ogre and its spawn point (Greyhaven content).
const OGRE: u32 = 710;
const OGRE_AT: [f32; 2] = [7_450.0, 18_150.0];
/// Yaws: 0 faces +Z, a quarter turn +X.
const SOUTH: u16 = 32_768;
const WEST: u16 = 49_152;

/// The brawl's players by ID and feet XZ: 1 provokes the ogre from 3 m south and later runs
/// on south past 2, who stands in the ogre's way; 3 walks into the ogre's side from the east.
const OGRE_BRAWL_PLAYERS: [(u32, [f32; 2]); 3] = [
    (1, [OGRE_AT[0], OGRE_AT[1] - 300.0]),
    (2, [OGRE_AT[0] + 20.0, OGRE_AT[1] - 800.0]),
    (3, [OGRE_AT[0] + 600.0, OGRE_AT[1]]),
];

/// The brawl's commands at `tick`, as `(player, command)`.
fn ogre_brawl_commands(tick: usize) -> Vec<(u32, ZoneCommand)> {
    let walk = |facing| ZoneCommand::Move {
        forward: 1,
        strafe: 0,
        facing,
    };
    let stand = |facing| ZoneCommand::Move {
        forward: 0,
        strafe: 0,
        facing,
    };
    let ogre = Some(EntityRef::Creature(CreatureId::new(OGRE)));
    match tick {
        0 => vec![
            (1, ZoneCommand::SelectTarget(ogre)),
            (1, ZoneCommand::StartAttack),
            (3, walk(WEST)),
        ],
        60 => vec![(1, walk(SOUTH)), (3, stand(WEST))],
        110 => vec![(1, stand(0))],
        200 => vec![(3, walk(WEST))],
        240 => vec![(3, stand(WEST))],
        _ => Vec::new(),
    }
}

/// The ogre brawl's initial zone: Greyhaven with its three players placed by the ogre.
fn ogre_brawl_zone() -> Result<ZoneSimulation, Box<dyn Error>> {
    placed_zone(&OGRE_BRAWL_PLAYERS)
}

/// [`placed_zone`] with every player at `level` and full health.
fn placed_zone_at_level(
    players: &[(u32, [f32; 2])],
    level: u8,
) -> Result<ZoneSimulation, Box<dyn Error>> {
    let zone = placed_zone(players)?;
    let mut state = zone.snapshot()?;
    for player in &mut state.players {
        player.combat.level = level;
        player.combat.health = mmorpg_core::unit::player_max_health(level);
    }
    Ok(ZoneSimulation::from_snapshot(
        state,
        std::sync::Arc::clone(zone.content()),
    )?)
}

/// Greyhaven with `players` placed at their feet XZ, standing on the ground.
fn placed_zone(players: &[(u32, [f32; 2])]) -> Result<ZoneSimulation, Box<dyn Error>> {
    let content = mmorpg_core::greyhaven_vale::content();
    let mut zone = ZoneSimulation::with_content(ZoneId::new(1), std::sync::Arc::clone(&content))?;
    for &(player, _) in players {
        zone.add_player(player)?;
    }
    let mut state = zone.snapshot()?;
    for &(player, [x, z]) in players {
        let snapshot = state
            .players
            .iter_mut()
            .find(|snapshot| snapshot.player_id == player)
            .ok_or("missing brawl player")?;
        #[allow(clippy::cast_precision_loss)]
        let y = zone.ground_probe(x as i32, z as i32) as f32 + 90.0;
        snapshot.position = [x, y, z];
    }
    Ok(ZoneSimulation::from_snapshot(state, content)?)
}

impl Workload {
    #[must_use]
    pub fn name(&self) -> &str {
        match self {
            Self::Bots(scenario) => &scenario.name,
            Self::Greyhaven(fixture) => fixture.name,
            Self::OgreBrawl => "vale-ogre-brawl",
            Self::MillHoist => "vale-mill-hoist",
            Self::MillRamps => "vale-mill-ramps",
            Self::ChargePath => "vale-charge-path",
        }
    }

    #[must_use]
    pub fn ticks(&self) -> usize {
        match self {
            Self::Bots(scenario) => usize::try_from(scenario.ticks).expect("tick count fits"),
            Self::Greyhaven(_) => greyhaven_workload::TICKS,
            Self::OgreBrawl => OGRE_BRAWL_TICKS,
            Self::MillHoist => MILL_HOIST_TICKS,
            Self::MillRamps => MILL_RAMPS_TICKS,
            Self::ChargePath => CHARGE_PATH_TICKS,
        }
    }

    /// Why physics-engine cannot run this workload, if it cannot: the capability it lacks.
    #[must_use]
    pub const fn engine_unsupported(&self) -> Option<&'static str> {
        match self {
            Self::MillHoist => Some("moving support: a platform that carries characters"),
            Self::Bots(_)
            | Self::Greyhaven(_)
            | Self::OgreBrawl
            | Self::MillRamps
            | Self::ChargePath => None,
        }
    }
}

/// The bot scenarios the A/B comparison replays: scripted movement through the hub and
/// gates, and a combat walk through the woods and its terrain.
pub const BOT_SCENARIOS: [&str; 3] = [
    include_str!("../../../mmorpg-scenarios/scenarios/bots/two-bots-move.toml"),
    include_str!("../../../mmorpg-scenarios/scenarios/bots/vale-hub-walk.toml"),
    include_str!("../../../mmorpg-scenarios/scenarios/bots/vale-wolf-hunt.toml"),
];

/// Every workload, bot scenarios first, then the Greyhaven combat fixtures.
pub fn workloads() -> Result<Vec<Workload>, String> {
    let mut workloads: Vec<_> = BOT_SCENARIOS
        .iter()
        .map(|source| BotScenario::parse(source).map(Workload::Bots))
        .collect::<Result<_, _>>()?;
    workloads.extend(greyhaven_workload::FIXTURES.map(Workload::Greyhaven));
    workloads.push(Workload::OgreBrawl);
    workloads.push(Workload::MillHoist);
    workloads.push(Workload::MillRamps);
    workloads.push(Workload::ChargePath);
    Ok(workloads)
}

/// A workload in progress on one zone.
pub struct Run {
    pub zone: ZoneSimulation,
    workload: Workload,
    /// Each bot's player once joined, and its next command sequence.
    players: Vec<Option<PlayerId>>,
    sequences: Vec<u32>,
    next_player: PlayerId,
    next_step: usize,
    /// Completed ticks.
    pub tick: usize,
    /// Each applied bot command's outcome, in order: `applied` or the zone's refusal.
    pub outcomes: Vec<String>,
}

impl Run {
    /// Builds the workload's initial zone on the current thread's backend.
    pub fn new(workload: &Workload) -> Result<Self, Box<dyn Error>> {
        let (zone, bots) = match workload {
            Workload::Bots(scenario) => (
                ZoneSimulation::with_content(
                    ZoneId::new(scenario.zone),
                    mmorpg_core::greyhaven_vale::content(),
                )?,
                scenario.bots.len(),
            ),
            Workload::Greyhaven(fixture) => (greyhaven_workload::initial_zone(*fixture)?, 0),
            Workload::OgreBrawl => (ogre_brawl_zone()?, 0),
            Workload::MillHoist => (mill_hoist_zone()?, 0),
            Workload::MillRamps => (placed_zone(&MILL_RAMPS_PLAYERS)?, 0),
            Workload::ChargePath => (placed_zone_at_level(&CHARGE_PATH_PLAYERS, 6)?, 0),
        };
        Ok(Self {
            zone,
            workload: workload.clone(),
            players: vec![None; bots],
            sequences: vec![1; bots],
            next_player: 1,
            next_step: 0,
            tick: 0,
            outcomes: Vec::new(),
        })
    }

    /// The same run restored from the zone's canonical snapshot (Atlas 2.1.5): physics is
    /// rebuilt from game state on the current thread's backend.
    pub fn recovered(&self) -> Result<Self, Box<dyn Error>> {
        Ok(Self {
            zone: ZoneSimulation::from_snapshot(
                self.zone.snapshot()?,
                std::sync::Arc::clone(self.zone.content()),
            )?,
            workload: self.workload.clone(),
            players: self.players.clone(),
            sequences: self.sequences.clone(),
            next_player: self.next_player,
            next_step: self.next_step,
            tick: self.tick,
            outcomes: self.outcomes.clone(),
        })
    }

    /// Whether every tick of the workload has run.
    #[must_use]
    pub fn finished(&self) -> bool {
        self.tick >= self.workload.ticks()
    }

    /// The player a bot joined as.
    #[must_use]
    pub fn player(&self, bot: usize) -> Option<PlayerId> {
        self.players.get(bot).copied().flatten()
    }

    /// Applies this tick's inputs, then advances the zone one tick.
    pub fn advance(&mut self) -> Result<(), Box<dyn Error>> {
        self.inputs()?;
        self.zone.advance_tick()?;
        self.tick += 1;
        Ok(())
    }

    /// Applies this tick's inputs without advancing (the timing example measures them with
    /// the tick).
    pub fn inputs(&mut self) -> Result<(), Box<dyn Error>> {
        match &self.workload {
            Workload::Greyhaven(fixture) => {
                greyhaven_workload::commands(&mut self.zone, *fixture, self.tick)?;
            }
            Workload::OgreBrawl
            | Workload::MillHoist
            | Workload::MillRamps
            | Workload::ChargePath => {
                let commands = match self.workload {
                    Workload::OgreBrawl => ogre_brawl_commands(self.tick),
                    Workload::MillHoist => mill_hoist_commands(self.tick),
                    Workload::ChargePath => charge_path_commands(self.tick),
                    _ => mill_ramps_commands(self.tick),
                };
                for (player, command) in commands {
                    let sequence = self.next_sequence(player);
                    let outcome = match self.zone.apply_command(player, sequence, command) {
                        Ok(()) => "applied".to_owned(),
                        Err(error) => format!("refused: {error}"),
                    };
                    self.outcomes.push(outcome);
                }
            }
            Workload::Bots(scenario) => {
                let scenario = scenario.clone();
                while let Some(step) = scenario.steps.get(self.next_step) {
                    if usize::try_from(step.tick)? != self.tick {
                        break;
                    }
                    self.apply(step)?;
                    self.next_step += 1;
                }
            }
        }
        Ok(())
    }

    /// The next command sequence of a scripted (non-bot) player.
    fn next_sequence(&mut self, player: PlayerId) -> u32 {
        let index = usize::try_from(player).expect("player IDs fit") - 1;
        if self.sequences.len() <= index {
            self.sequences.resize(index + 1, 1);
        }
        let sequence = self.sequences[index];
        self.sequences[index] += 1;
        sequence
    }

    fn apply(&mut self, step: &Step) -> Result<(), Box<dyn Error>> {
        let command = match &step.action {
            Action::Join => {
                let player = self.next_player;
                self.next_player += 1;
                self.zone.add_player(player)?;
                self.players[step.bot] = Some(player);
                return Ok(());
            }
            &Action::Move {
                forward,
                strafe,
                facing,
            } => ZoneCommand::Move {
                forward,
                strafe,
                facing,
            },
            Action::Jump => ZoneCommand::Jump,
            Action::SelectTarget(unit) => ZoneCommand::SelectTarget(match unit {
                Unit::None => None,
                Unit::Bot(bot) => Some(EntityRef::Player(
                    self.player(*bot).ok_or("target bot has not joined")?,
                )),
                Unit::Creature(id) => Some(EntityRef::Creature(CreatureId::new(*id))),
                Unit::Npc(id) => Some(EntityRef::Npc(NpcId::new(*id))),
            }),
            Action::StartAttack => ZoneCommand::StartAttack,
            Action::StopAttack => ZoneCommand::StopAttack,
            Action::ReleaseSpirit => ZoneCommand::ReleaseSpirit,
        };
        let player = self.player(step.bot).ok_or("bot has not joined")?;
        let sequence = self.sequences[step.bot];
        self.sequences[step.bot] += 1;
        let outcome = match self.zone.apply_command(player, sequence, command) {
            Ok(()) => "applied".to_owned(),
            Err(error) => format!("refused: {error}"),
        };
        self.outcomes.push(outcome);
        Ok(())
    }
}
