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
    /// A swing (#517) toward a fixed facing, or aimed at a unit as the bot runner aims it.
    Swing {
        facing: Option<u16>,
        at: Option<Unit>,
    },
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

/// The yaw from `player` toward `target` in the player's projection, as the bot runner aims a
/// swing (#517); the player's own facing while either is out of view.
fn aim(zone: &ZoneSimulation, player: PlayerId, target: Option<EntityRef>) -> u16 {
    let Ok(view) = zone.snapshot_for_player(player) else {
        return 0;
    };
    let at = |entity: EntityRef| view.entities.iter().find(|unit| unit.entity() == entity);
    let Some(me) = at(EntityRef::Player(player)) else {
        return 0;
    };
    target
        .and_then(at)
        .and_then(|unit| {
            mmorpg_core::trig::yaw_from_vector(
                unit.position[0] - me.position[0],
                unit.position[2] - me.position[2],
            )
        })
        .unwrap_or(me.facing)
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
                "swing" => Action::Swing {
                    facing: step
                        .contains_key("facing")
                        .then(|| integer(step, "facing"))
                        .transpose()?,
                    at: step
                        .contains_key("entity")
                        .then(|| unit(text(step, "entity")?))
                        .transpose()?,
                },
                "release_spirit" => Action::ReleaseSpirit,
                other => return Err(format!("action {other} is not replayed")),
            };
            let (tick, bot) = (integer(step, "tick")?, bot(text(step, "bot")?)?);
            // A repeating swing unrolls as the bot runner unrolls it (#517).
            if step.contains_key("until") {
                let (until, every): (u64, usize) =
                    (integer(step, "until")?, integer(step, "every")?);
                for tick in (tick..until).step_by(every) {
                    steps.push(Step {
                        tick,
                        bot,
                        action: action.clone(),
                    });
                }
            } else {
                steps.push(Step { tick, bot, action });
            }
        }
        steps.sort_by_key(|step| step.tick);
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
    MillHoist,
    /// Charges through Wolfrun Woods (#57): four level-6 Wardens charge at wolves among the
    /// trunks; a charge needs a path as wide as the body (`World::cast_shape`).
    ChargePath,
    /// The Redbrand watch post (#305): an arcanist draws the camp's bandits past the post and
    /// blasts them there, breaking pieces whose colliders leave both physical worlds.
    WatchPost,
    /// The mill dock's ramps (#59): one player walks up the cart ramp onto the dock and back
    /// down and across it, another walks into the steep chute.
    MillRamps,
    /// Millbrook's heavy crates (#58): two Wardens Shield Bash them across the yard, then one
    /// walks into a crate where it rests.
    CrateShove,
    /// Greyhaven Outpost's crates (#288): two players smash crates 900 and 901, whose pieces
    /// fly, settle and despawn as debris on each backend.
    CrateSmash,
    /// Magic missiles in Wolfrun Woods (#289): four level-6 Arcanists Firebolt wolves among the
    /// trunks (homing missiles), then cast free Firebolts along their facing into the woods.
    Missiles,
    /// Running jumps with authored momentum (#413): a Warden and a Ranger in Greyhaven
    /// Outpost run, jump, let go or turn mid-air, and land, under [`TRAVERSAL_GRAPH`], whose
    /// airborne phase keeps the velocity contact realized and steers at most 6 m/s².
    Traversal,
    /// Climbing and mantling (#490): one player jumps at a highland crag, climbs, lets go,
    /// grabs it again and climbs to its top; another mantles the mill's loading dock.
    Climb,
    /// Dodge rolls (#516): one player rolls back, forward and diagonally in Greyhaven Outpost,
    /// another into a highland crag and back out, a third back off the mill's loading dock.
    Roll,
    /// Swimming in Stillwater Lake (#274, #537): one player wades in from the west shore, swims
    /// out and dives toward the basin's floor; another starts on the floor in the deep water,
    /// presses down onto it, glides east along it and rises to the surface.
    Swim,
}

/// The swim workload's length.
pub const SWIM_TICKS: usize = mmorpg_core::golden::t30(360) as usize;

/// The wader on the lake's west shore and the diver on the basin's floor near its centre, by
/// ID and feet XZ.
const SWIM_PLAYERS: [(u32, [f32; 2]); 2] = [(1, [2_900.0, -5_500.0]), (2, [5_300.0, -5_400.0])];

/// The swim's commands at `tick` (30 Hz).
fn swim_commands(tick: usize) -> Vec<(u32, ZoneCommand)> {
    // East, along the lake's long axis.
    let east = |forward| ZoneCommand::Move {
        forward,
        strafe: 0,
        facing: 16_384,
    };
    match tick {
        0 => vec![(1, east(1)), (2, ZoneCommand::Rise(-1))],
        90 => vec![(1, ZoneCommand::Rise(-1))],
        150 => vec![(2, east(1))],
        240 => vec![(1, ZoneCommand::Rise(0)), (2, ZoneCommand::Rise(1))],
        300 => vec![(1, ZoneCommand::Rise(1)), (2, east(0))],
        _ => Vec::new(),
    }
}

/// The roll workload's length.
pub const ROLL_TICKS: usize = mmorpg_core::golden::t30(200) as usize;

/// The plaza roller, the crag roller and the dock roller, by ID, feet XZ and height above the
/// terrain: the third stands on the dock's 1.5 m deck.
pub const ROLL_PLAYERS: [(u32, [f32; 2], f32); 3] = [
    (1, [2_200.0, 850.0], 0.0),
    (2, [1_500.0, -28_900.0], 0.0),
    (3, [8_250.0, 1_950.0], 150.0),
];

/// The roll commands at `tick` (30 Hz). A roll lasts 18 of these ticks and its cooldown 30.
fn roll_commands(tick: usize) -> Vec<(u32, ZoneCommand)> {
    let held = |forward, strafe| ZoneCommand::Move {
        forward,
        strafe,
        facing: 32_768,
    };
    match tick {
        0 => vec![(1, held(0, 0)), (2, held(1, 0)), (3, held(0, 0))],
        // Into the crag at full tilt; standing still, back off the dock.
        3 => vec![(2, ZoneCommand::Roll)],
        5 => vec![(1, ZoneCommand::Roll)],
        10 => vec![(3, ZoneCommand::Roll)],
        30 => vec![(2, held(0, 0))],
        60 => vec![(1, held(1, 0))],
        62 => vec![(1, ZoneCommand::Roll)],
        // Keys released mid-roll: the roll carries on.
        64 => vec![(1, held(0, 0))],
        // Standing still, back out from the crag.
        80 => vec![(2, ZoneCommand::Roll)],
        120 => vec![(1, held(1, 1))],
        122 => vec![(1, ZoneCommand::Roll)],
        140 => vec![(1, held(0, 0))],
        _ => Vec::new(),
    }
}

/// The roll workload's zone: [`ROLL_PLAYERS`] placed with their feet on the terrain or the dock.
fn roll_zone() -> Result<ZoneSimulation, Box<dyn Error>> {
    let content = mmorpg_core::greyhaven_vale::content();
    let mut zone = ZoneSimulation::with_content(ZoneId::new(1), std::sync::Arc::clone(&content))?;
    for &(player, ..) in &ROLL_PLAYERS {
        zone.add_player(player)?;
    }
    let mut state = zone.snapshot()?;
    for &(player, [x, z], lift) in &ROLL_PLAYERS {
        let snapshot = state
            .players
            .iter_mut()
            .find(|snapshot| snapshot.player_id == player)
            .ok_or("missing roll player")?;
        #[allow(clippy::cast_precision_loss)]
        let y = zone.ground_probe(x as i32, z as i32) as f32 + lift + 90.0;
        snapshot.position = [x, y, z];
    }
    Ok(ZoneSimulation::from_snapshot(state, content)?)
}

/// The climb's length.
pub const CLIMB_TICKS: usize = mmorpg_core::golden::t30(520) as usize;

/// The climber below the crag flanking the gorge's top and the mantler in the mill's yard, by
/// ID and feet XZ.
const CLIMB_PLAYERS: [(u32, [f32; 2]); 2] = [(1, [1_500.0, -28_900.0]), (2, [8_250.0, 2_300.0])];

/// The climb's commands at `tick` (30 Hz): both walk north into their walls and jump; the
/// climber climbs, lets go (a jump, releasing the keys so it drops to the ground), grabs again
/// (another jump) and climbs on to the top; the mantler stands on the dock.
fn climb_commands(tick: usize) -> Vec<(u32, ZoneCommand)> {
    let hold = |forward| ZoneCommand::Move {
        forward,
        strafe: 0,
        facing: 32_768,
    };
    match tick {
        0 => vec![(1, hold(1)), (2, hold(1))],
        20 => vec![(1, ZoneCommand::Jump), (2, ZoneCommand::Jump)],
        50 => vec![(2, hold(0))],
        80 => vec![(1, ZoneCommand::Jump), (1, hold(0))],
        110 => vec![(1, hold(1))],
        112 => vec![(1, ZoneCommand::Jump)],
        // Mid-mantle at the top: the mantle carries on and the climber stands on the crag.
        460 => vec![(1, hold(0))],
        _ => Vec::new(),
    }
}

/// The traversal workload's graph: momentum through the air, slowed landings.
pub static TRAVERSAL_GRAPH: mmorpg_core::action::ActionGraph = mmorpg_core::action::ActionGraph {
    movement: &[
        mmorpg_core::action::MovementRule {
            action: mmorpg_core::action::ActionId::Airborne,
            phase: mmorpg_core::action::Phase::Active,
            conditions: &[],
            movement: mmorpg_core::action::Movement {
                steering_percent: 100,
                momentum_percent: 100,
                max_acceleration_cms2: Some(600),
            },
        },
        mmorpg_core::action::MovementRule {
            action: mmorpg_core::action::ActionId::Landing,
            phase: mmorpg_core::action::Phase::Recovery,
            conditions: &[],
            movement: mmorpg_core::action::Movement {
                steering_percent: 50,
                momentum_percent: 0,
                max_acceleration_cms2: None,
            },
        },
    ],
    ..mmorpg_core::action::BASELINE
};

/// The traversal workload's length.
pub const TRAVERSAL_TICKS: usize = mmorpg_core::golden::t30(240) as usize;

/// A Warden and a Ranger in the outpost, by ID and feet XZ.
const TRAVERSAL_PLAYERS: [(u32, [f32; 2]); 2] = [(1, [2_200.0, 850.0]), (2, [2_310.0, 850.0])];

/// The traversal commands at `tick` (30 Hz): run, jump, let go (player 1) or turn back
/// (player 2) in the air, land, and again.
fn traversal_commands(tick: usize) -> Vec<(u32, ZoneCommand)> {
    let run = |forward, facing| ZoneCommand::Move {
        forward,
        strafe: 0,
        facing,
    };
    let (south, east, north) = (32_768, 16_384, 0);
    match tick {
        0 => vec![
            (1, ZoneCommand::ChooseClass { class: 0, sex: 0 }),
            (2, ZoneCommand::ChooseClass { class: 1, sex: 1 }),
        ],
        1 => vec![(1, run(1, south)), (2, run(1, south))],
        20 | 100 => vec![(1, ZoneCommand::Jump), (2, ZoneCommand::Jump)],
        22 | 102 => vec![(1, run(0, south)), (2, run(1, north))],
        60 => vec![(1, run(1, east)), (2, run(1, east))],
        150 => vec![(1, run(0, east)), (2, run(0, east))],
        _ => Vec::new(),
    }
}

/// The crate-smash workload's length: the crates break, and their debris settles and
/// despawns (5 s).
pub const CRATE_SMASH_TICKS: usize = mmorpg_core::golden::t30(360) as usize;

/// Two players 1.5 m south of crates 900 and 901.
const CRATE_SMASH_PLAYERS: [(u32, [f32; 2]); 2] = [(1, [2_200.0, 850.0]), (2, [2_310.0, 850.0])];

/// The crate-smash commands: face north, target the crate, swing north at it (#517) until it
/// breaks.
fn crate_smash_commands(tick: usize) -> Vec<(u32, ZoneCommand)> {
    [(1, 900), (2, 901)]
        .into_iter()
        .filter_map(|(player, prop)| match tick {
            0 => Some((
                player,
                ZoneCommand::Move {
                    forward: 0,
                    strafe: 0,
                    facing: 0,
                },
            )),
            1 => Some((
                player,
                ZoneCommand::SelectTarget(Some(EntityRef::Creature(CreatureId::new(prop)))),
            )),
            2..CRATE_SMASH_TICKS => Some((player, ZoneCommand::Swing { facing: 0 })),
            _ => None,
        })
        .collect()
}

/// The missile workload's length.
pub const MISSILES_TICKS: usize = mmorpg_core::golden::t30(300) as usize;

/// The missile commands: Arcanists, wolves as targets, homing Firebolts, then free ones.
fn missiles_commands(tick: usize) -> Vec<(u32, ZoneCommand)> {
    let wolves = [102, 103, 101, 107];
    let firebolt = |target| ZoneCommand::UseAbility {
        ability: mmorpg_core::ability::ids::FIREBOLT.get(),
        target,
    };
    (1..=4)
        .flat_map(|player: u32| {
            let wolf = wolves[usize::try_from(player - 1).expect("four players")];
            let facing = u16::try_from(player * 16_384 - 8_192).expect("a facing");
            match tick {
                0 => vec![(player, ZoneCommand::ChooseClass { class: 2, sex: 0 })],
                1 => vec![(
                    player,
                    ZoneCommand::SelectTarget(Some(EntityRef::Creature(CreatureId::new(wolf)))),
                )],
                2 => vec![(player, firebolt(None))],
                150 => vec![
                    (player, ZoneCommand::SelectTarget(None)),
                    (
                        player,
                        ZoneCommand::Move {
                            forward: 0,
                            strafe: 0,
                            facing,
                        },
                    ),
                ],
                151 => vec![(player, firebolt(None))],
                _ => Vec::new(),
            }
        })
        .collect()
}

/// The crate workload's length.
pub const CRATE_SHOVE_TICKS: usize = mmorpg_core::golden::t30(180) as usize;

/// Two Wardens 2 m south of heavy crates 917 and 918.
const CRATE_SHOVE_PLAYERS: [(u32, [f32; 2]); 2] =
    [(1, [7_400.0, 3_000.0]), (2, [7_550.0, 3_000.0])];

/// Two level-6 Wardens with full rage by the crates.
fn crate_shove_zone() -> Result<ZoneSimulation, Box<dyn Error>> {
    let zone = placed_zone_at_level(&CRATE_SHOVE_PLAYERS, 6)?;
    let mut state = zone.snapshot()?;
    for player in &mut state.players {
        player.combat.abilities.class = Some(mmorpg_core::ClassChoice {
            class: mmorpg_core::PlayerClass::Warden,
            sex: mmorpg_core::Sex::Female,
        });
        player.combat.abilities.resource = 100;
    }
    Ok(ZoneSimulation::from_snapshot(
        state,
        std::sync::Arc::clone(zone.content()),
    )?)
}

/// The crate commands: Wardens facing north, the crates as targets, bashes, then player 2
/// walks north into its crate.
fn crate_shove_commands(tick: usize) -> Vec<(u32, ZoneCommand)> {
    let stand = |forward| ZoneCommand::Move {
        forward,
        strafe: 0,
        facing: 0,
    };
    let crates = [(1, 917), (2, 918)];
    crates
        .into_iter()
        .flat_map(|(player, prop)| match tick {
            0 => vec![(player, stand(0))],
            1 => vec![(
                player,
                ZoneCommand::SelectTarget(Some(EntityRef::Creature(CreatureId::new(prop)))),
            )],
            2 => vec![(
                player,
                ZoneCommand::UseAbility {
                    ability: mmorpg_core::ability::ids::SHIELD_BASH.get(),
                    target: None,
                },
            )],
            60 if player == 2 => vec![(player, stand(1))],
            150 if player == 2 => vec![(player, stand(0))],
            _ => Vec::new(),
        })
        .collect()
}

/// The charge workload's length.
pub const CHARGE_PATH_TICKS: usize = mmorpg_core::golden::t30(240) as usize;

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

/// The watch post workload's length.
pub const WATCH_POST_TICKS: usize = mmorpg_core::golden::t30(420) as usize;

/// The arcanist just north of the watch post, inside the camp, and a ranger beside it.
const WATCH_POST_PLAYERS: [(u32, [f32; 2]); 2] = [(1, [700.0, -7_350.0]), (2, [300.0, -7_400.0])];

/// The watch post commands: classes, the nearest bandits as targets, Fire Blasts and arrows
/// fired where the standing players turn to face them, and the ranger's swings at its bandit
/// (#517).
fn watch_post_commands(tick: usize, zone: &ZoneSimulation) -> Vec<(u32, ZoneCommand)> {
    let creature = |id| Some(EntityRef::Creature(CreatureId::new(id)));
    let face = |player, id| ZoneCommand::Move {
        forward: 0,
        strafe: 0,
        facing: aim(zone, player, creature(id)),
    };
    let ability = |ability: mmorpg_core::AbilityId, target| ZoneCommand::UseAbility {
        ability: ability.get(),
        target,
    };
    use mmorpg_core::ability::ids;
    match tick {
        0 => vec![
            (1, ZoneCommand::ChooseClass { class: 2, sex: 0 }),
            (2, ZoneCommand::ChooseClass { class: 1, sex: 1 }),
        ],
        1 => vec![
            (1, ZoneCommand::SelectTarget(creature(401))),
            (2, ZoneCommand::SelectTarget(creature(400))),
        ],
        // The bandits run at them past the post; blast whoever is at the walls.
        30 | 340 => vec![
            (1, face(1, 401)),
            (1, ability(ids::FIRE_BLAST, creature(401))),
        ],
        60 | 120 | 200 => vec![
            (2, face(2, 400)),
            (2, ability(ids::AIMED_SHOT, creature(400))),
        ],
        2..WATCH_POST_TICKS => vec![(
            2,
            ZoneCommand::Swing {
                facing: aim(zone, 2, creature(400)),
            },
        )],
        _ => Vec::new(),
    }
}

/// The ramp walk's length.
pub const MILL_RAMPS_TICKS: usize = mmorpg_core::golden::t30(240) as usize;

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
pub const MILL_HOIST_TICKS: usize = mmorpg_core::golden::t30(640) as usize;

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
    let after = |ticks| up + usize::try_from(mmorpg_core::golden::t30(ticks)).expect("ticks fit");
    if tick == after(10) {
        vec![(1, walk(1))]
    } else if tick == after(30) {
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
    let feet = [
        x as f32,
        (mmorpg_core::greyhaven_vale::basin::TERRACE_Y + 90) as f32,
        z as f32,
    ];
    state.players[0].position = feet;
    Ok(ZoneSimulation::from_snapshot(state, content)?)
}

/// The ogre brawl's script length.
pub const OGRE_BRAWL_TICKS: usize = mmorpg_core::golden::t30(360) as usize;
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
        0 => vec![(1, ZoneCommand::SelectTarget(ogre)), (3, walk(WEST))],
        60 => vec![(1, walk(SOUTH)), (3, stand(WEST))],
        110 => vec![(1, stand(0))],
        200 => vec![(3, walk(WEST))],
        240 => vec![(3, stand(WEST))],
        // While 1 stands facing the ogre to its north it keeps swinging (#517).
        1..60 | 111..OGRE_BRAWL_TICKS => vec![(1, ZoneCommand::Swing { facing: 0 })],
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

/// [`placed_zone`] running [`TRAVERSAL_GRAPH`].
fn traversal_zone(players: &[(u32, [f32; 2])]) -> Result<ZoneSimulation, Box<dyn Error>> {
    let mut zone = placed_zone(players)?;
    zone.set_action_graph(&TRAVERSAL_GRAPH)
        .map_err(|error| format!("{error:?}"))?;
    Ok(zone)
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
            Self::WatchPost => "vale-watch-post",
            Self::CrateShove => "vale-crate-shove",
            Self::CrateSmash => "vale-crate-smash",
            Self::Missiles => "vale-missiles",
            Self::Traversal => "vale-traversal",
            Self::Climb => "vale-climb",
            Self::Roll => "vale-roll",
            Self::Swim => "vale-swim",
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
            Self::WatchPost => WATCH_POST_TICKS,
            Self::CrateShove => CRATE_SHOVE_TICKS,
            Self::CrateSmash => CRATE_SMASH_TICKS,
            Self::Missiles => MISSILES_TICKS,
            Self::Traversal => TRAVERSAL_TICKS,
            Self::Climb => CLIMB_TICKS,
            Self::Roll => ROLL_TICKS,
            Self::Swim => SWIM_TICKS,
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
    workloads.push(Workload::WatchPost);
    workloads.push(Workload::CrateShove);
    workloads.push(Workload::CrateSmash);
    workloads.push(Workload::Missiles);
    workloads.push(Workload::Traversal);
    workloads.push(Workload::Climb);
    workloads.push(Workload::Roll);
    workloads.push(Workload::Swim);
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
            Workload::WatchPost => (placed_zone_at_level(&WATCH_POST_PLAYERS, 6)?, 0),
            Workload::CrateShove => (crate_shove_zone()?, 0),
            Workload::CrateSmash => (placed_zone_at_level(&CRATE_SMASH_PLAYERS, 6)?, 0),
            Workload::Missiles => (placed_zone_at_level(&CHARGE_PATH_PLAYERS, 6)?, 0),
            Workload::Traversal => (traversal_zone(&TRAVERSAL_PLAYERS)?, 0),
            Workload::Climb => (placed_zone(&CLIMB_PLAYERS)?, 0),
            Workload::Roll => (roll_zone()?, 0),
            Workload::Swim => (placed_zone(&SWIM_PLAYERS)?, 0),
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
        let mut zone = ZoneSimulation::from_snapshot(
            self.zone.snapshot()?,
            std::sync::Arc::clone(self.zone.content()),
        )?;
        // The graph is zone setup, not canonical state.
        zone.set_action_graph(self.zone.action_graph())
            .map_err(|error| format!("{error:?}"))?;
        Ok(Self {
            zone,
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
            | Workload::ChargePath
            | Workload::WatchPost
            | Workload::CrateShove
            | Workload::CrateSmash
            | Workload::Missiles
            | Workload::Traversal
            | Workload::Climb
            | Workload::Roll
            | Workload::Swim => {
                // The scripts are written in 30 Hz ticks: they run at the same moments at TICK_HZ.
                let scale = usize::from(mmorpg_core::TICK_HZ) / 30;
                let at = if self.tick.is_multiple_of(scale) {
                    self.tick / scale
                } else {
                    usize::MAX
                };
                let commands = match self.workload {
                    Workload::OgreBrawl => ogre_brawl_commands(at),
                    // Timed from the hoist's own schedule, in zone ticks.
                    Workload::MillHoist => mill_hoist_commands(self.tick),
                    Workload::ChargePath => charge_path_commands(at),
                    Workload::WatchPost => watch_post_commands(at, &self.zone),
                    Workload::CrateShove => crate_shove_commands(at),
                    Workload::CrateSmash => crate_smash_commands(at),
                    Workload::Missiles => missiles_commands(at),
                    Workload::Traversal => traversal_commands(at),
                    Workload::Climb => climb_commands(at),
                    Workload::Roll => roll_commands(at),
                    Workload::Swim => swim_commands(at),
                    _ => mill_ramps_commands(at),
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
            Action::Swing { facing, at } => {
                let player = self.player(step.bot).ok_or("bot has not joined")?;
                let target = match at {
                    Some(Unit::Bot(bot)) => self.player(*bot).map(EntityRef::Player),
                    Some(Unit::Creature(id)) => Some(EntityRef::Creature(CreatureId::new(*id))),
                    Some(Unit::Npc(id)) => Some(EntityRef::Npc(NpcId::new(*id))),
                    Some(Unit::None) | None => None,
                };
                ZoneCommand::Swing {
                    facing: facing.unwrap_or_else(|| aim(&self.zone, player, target)),
                }
            }
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
