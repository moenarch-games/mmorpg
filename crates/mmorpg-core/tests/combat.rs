//! Deterministic combat scenarios through the public zone API: targeting,
//! auto-attack, damage, tapping, death, corpses, respawn, assist, release
//! spirit, leash, evade and refused intents.
mod support;

use std::sync::Arc;

use mmorpg_core::unit::{COMBAT_LINGER_TICKS, CORPSE_TICKS, PLAYER_SWING_TICKS};
use mmorpg_core::{
    CREATURE_EVADE_SPEED_UNITS_PER_TICK, CanonicalCreatureSnapshot, CreatureAi, CreatureId,
    CreatureLife, EntityKind, EntityRef, ErrorCode, NpcId, ThreatEntry, ZoneCommand, ZoneEvent,
    ZoneId, ZoneSimulation,
};
use support::arena::{self, BOAR, GRAVEYARD, NORTHWARD, WOLF};

const PLAYER: EntityRef = EntityRef::Player(1);
const FIRST: CreatureId = CreatureId::new(1);
const SECOND: CreatureId = CreatureId::new(2);
const WEST: u16 = 49_152;

/// Drives one player through a zone with monotonic sequences.
struct Session {
    zone: ZoneSimulation,
    sequence: u32,
    events: Vec<(u64, ZoneEvent)>,
}

impl Session {
    fn new(content: Arc<mmorpg_core::ZoneContent>) -> Self {
        let mut zone = ZoneSimulation::with_content(ZoneId::new(5), content).unwrap();
        zone.add_player(1).unwrap();
        Self {
            zone,
            sequence: 0,
            events: Vec::new(),
        }
    }

    fn send(&mut self, command: ZoneCommand) {
        self.sequence += 1;
        self.zone.apply_command(1, self.sequence, command).unwrap();
    }

    fn tick(&mut self) {
        self.zone.advance_tick().unwrap();
        let view = self.zone.snapshot_for_player(1).unwrap();
        let tick = self.zone.current_tick();
        self.events
            .extend(view.events.into_iter().map(|event| (tick, event)));
    }

    fn ticks(&mut self, count: u64) {
        for _ in 0..count {
            self.tick();
        }
    }

    /// Ticks until `done` holds, at most `limit` ticks; returns the tick.
    fn until(&mut self, limit: u64, done: impl Fn(&Self) -> bool) -> u64 {
        for _ in 0..limit {
            self.tick();
            if done(self) {
                return self.zone.current_tick();
            }
        }
        panic!("condition not reached within {limit} ticks");
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

    fn health(&self) -> u32 {
        self.zone.snapshot_for_player(1).unwrap().viewer.health
    }

    fn errors(&self) -> Vec<(u64, ErrorCode)> {
        self.events
            .iter()
            .filter_map(|(tick, event)| match event {
                ZoneEvent::Error { code, .. } => Some((*tick, *code)),
                _ => None,
            })
            .collect()
    }
}

/// A player walks to a wolf, targets it and auto-attacks until it dies; the
/// wolf is tapped, leaves a corpse and respawns after its timer.
#[test]
fn a_player_hunts_a_wolf_from_approach_to_respawn() {
    let content = arena::arena(
        vec![arena::wolf(30, [1, 2])],
        vec![arena::spawn(1, WOLF, [0, 2_000])],
        vec![],
    );
    let mut session = Session::new(content);
    let wolf = EntityRef::Creature(FIRST);

    // Walk north until the wolf notices the player (10 m aggro radius).
    session.send(arena::walk(NORTHWARD));
    let aggro = session.until(100, |session| {
        session.creature(FIRST).ai == CreatureAi::Engaged
    });
    let position = session.zone.snapshot().unwrap().players[0].position;
    assert!(
        position[2] >= 1_000 - 21,
        "aggro began at z = {}",
        position[2]
    );
    assert_eq!(
        session.creature(FIRST).threat,
        [ThreatEntry {
            entity: PLAYER,
            threat: 0
        }],
        "aggro puts the player on the threat table at tick {aggro}"
    );
    session.send(arena::stand(NORTHWARD));
    session.send(ZoneCommand::SelectTarget(Some(wolf)));
    session.send(ZoneCommand::StartAttack);
    session.tick();
    let view = session.zone.snapshot_for_player(1).unwrap();
    assert_eq!(view.viewer.target, Some(wolf));
    assert!(view.viewer.auto_attacking && view.viewer.in_combat);
    assert_eq!(
        view.target_of_target,
        Some(PLAYER),
        "the wolf targets the player"
    );
    assert_eq!(view.entities[0].entity(), PLAYER, "the viewer leads");
    assert_eq!(view.entities[1].entity(), wolf, "its target comes next");
    let record = &view.entities[1];
    assert_eq!((record.appearance, record.level), (WOLF.get(), 1));
    assert!(record.flags.hostile && record.flags.attackable && record.flags.in_combat);
    assert!(record.flags.targets_viewer && !record.flags.tapped_by_other);

    // Swing until the wolf dies.
    let died = session.until(3_000, |session| {
        !matches!(session.creature(FIRST).life, CreatureLife::Alive)
    });
    let dealt: u32 = session
        .events
        .iter()
        .filter_map(|(_, event)| match event {
            ZoneEvent::DamageDealt {
                source,
                target,
                amount,
                ..
            } => {
                assert_eq!((*source, *target), (PLAYER, wolf));
                Some(u32::from(*amount))
            }
            _ => None,
        })
        .sum();
    assert_eq!(dealt, 30, "damage dealt adds up to the wolf's health");
    let swings = session
        .events
        .iter()
        .filter(|(_, event)| {
            matches!(
                event,
                ZoneEvent::DamageDealt { source: PLAYER, .. }
                    | ZoneEvent::Miss { source: PLAYER, .. }
            )
        })
        .map(|(tick, _)| *tick)
        .collect::<Vec<_>>();
    assert!(
        swings
            .windows(2)
            .all(|pair| pair[1] - pair[0] == u64::from(PLAYER_SWING_TICKS)),
        "swings follow the swing timer: {swings:?}"
    );
    assert!(session.events.contains(&(
        died,
        ZoneEvent::Died {
            entity: wolf,
            killer: Some(PLAYER)
        }
    )));
    assert!(
        session
            .events
            .iter()
            .any(|(_, event)| matches!(event, ZoneEvent::DamageTaken { source, target: PLAYER, .. } if *source == wolf)),
        "the wolf fought back"
    );
    let corpse = session.creature(FIRST);
    assert_eq!(corpse.life, CreatureLife::Corpse { died_at: died });
    assert_eq!(
        corpse.tapped_by,
        Some(1),
        "the first damaging player tapped it"
    );
    assert_eq!((corpse.health, corpse.velocity), (0, [0; 3]));

    session.tick();
    let view = session.zone.snapshot_for_player(1).unwrap();
    assert!(
        !view.viewer.auto_attacking,
        "the target's death stops auto-attack"
    );
    assert_eq!(view.viewer.target, Some(wolf), "the corpse stays selected");
    let record = view
        .entities
        .iter()
        .find(|entity| entity.entity() == wolf)
        .unwrap();
    assert!(record.flags.dead && !record.flags.attackable && !record.flags.tapped_by_other);
    assert_eq!(record.health_percent, 0);
    assert_eq!(record.position, corpse.position);
    session.send(ZoneCommand::StartAttack);
    session.tick();
    assert_eq!(
        session.errors().last(),
        Some(&(died + 2, ErrorCode::TargetDead))
    );

    // The corpse disappears after 30 s and clears the selection.
    session.until(u64::from(CORPSE_TICKS), |session| {
        session.zone.current_tick() == died + u64::from(CORPSE_TICKS)
    });
    assert_eq!(
        session.creature(FIRST).life,
        CreatureLife::Despawned { died_at: died }
    );
    let view = session.zone.snapshot_for_player(1).unwrap();
    assert_eq!(view.viewer.target, None);
    assert!(
        view.entities
            .iter()
            .all(|entity| entity.kind != EntityKind::Creature)
    );

    // It respawns at its spawn point with full health 60 s after death.
    session.until(1_800, |session| session.zone.current_tick() == died + 1_800);
    let respawned = session.creature(FIRST);
    assert_eq!(respawned.life, CreatureLife::Alive);
    assert_eq!(respawned.health, 30);
    assert_eq!(respawned.position, [0, 45, 2_000]);
    assert_eq!(respawned.tapped_by, None);
    assert!(respawned.threat.is_empty());
}

/// Two wolves: one aggroes, its packmate assists, the player dies, both
/// evade home and reset, and the released spirit returns to the graveyard.
#[test]
fn assist_death_release_spirit_and_evade() {
    let content = arena::arena(
        vec![arena::wolf(80, [20, 30])],
        vec![
            arena::spawn(1, WOLF, [0, 2_000]),
            arena::spawn(2, WOLF, [600, 2_300]),
        ],
        vec![],
    );
    let mut session = Session::new(content);
    session.send(arena::walk(NORTHWARD));
    session.until(100, |session| {
        session.creature(FIRST).ai == CreatureAi::Engaged
    });
    assert_eq!(
        session.creature(SECOND).ai,
        CreatureAi::Engaged,
        "a packmate within 8 m assists"
    );
    session.send(arena::stand(NORTHWARD));
    session.send(ZoneCommand::SelectTarget(Some(EntityRef::Creature(FIRST))));
    session.send(ZoneCommand::StartAttack);

    let died = session.until(600, |session| session.health() == 0);
    assert!(
        session.events.contains(&(
            died,
            ZoneEvent::Died {
                entity: PLAYER,
                killer: session
                    .events
                    .iter()
                    .rev()
                    .find_map(|(_, event)| match event {
                        ZoneEvent::DamageTaken { source, .. } => Some(*source),
                        _ => None,
                    })
            }
        ))
    );
    let view = session.zone.snapshot_for_player(1).unwrap();
    assert!(view.viewer.dead && !view.viewer.auto_attacking);
    assert_eq!(view.entities[0].health_percent, 0);
    assert!(view.entities[0].flags.dead);
    for id in [FIRST, SECOND] {
        assert!(
            session.creature(id).threat.is_empty(),
            "the dead leave threat tables"
        );
    }
    let damaged = session.creature(FIRST);
    assert!(damaged.health < 80, "the player hit the first wolf");

    // Dead players can only release their spirit; they cannot move.
    let corpse = session.zone.snapshot().unwrap().players[0].position;
    session.send(arena::walk(NORTHWARD));
    session.send(ZoneCommand::StartAttack);
    session.send(ZoneCommand::SelectTarget(None));
    session.tick();
    assert_eq!(
        session.errors()[session.errors().len() - 2..],
        [
            (died + 1, ErrorCode::YouAreDead),
            (died + 1, ErrorCode::YouAreDead)
        ]
    );
    assert_eq!(session.zone.snapshot().unwrap().players[0].position, corpse);

    // Both wolves evade home, ignoring nothing because nobody attacks, and reset.
    session.tick();
    for id in [FIRST, SECOND] {
        assert!(matches!(
            session.creature(id).ai,
            CreatureAi::Evading { .. }
        ));
    }
    session.until(400, |session| {
        [FIRST, SECOND]
            .iter()
            .all(|&id| matches!(session.creature(id).ai, CreatureAi::Idle { .. }))
    });
    for (id, home) in [(FIRST, [0, 2_000]), (SECOND, [600, 2_300])] {
        let wolf = session.creature(id);
        assert_eq!(wolf.health, 80, "evading resets health");
        assert_eq!(wolf.tapped_by, None);
        let [x, _, z] = wolf.position;
        let (dx, dz) = (i64::from(x - home[0]), i64::from(z - home[1]));
        assert!(dx * dx + dz * dz <= 20 * 20, "{id:?} is home at {x}, {z}");
    }
    // Combat lingers 5 s after the killing blow, whether dead or alive.
    while session.zone.current_tick() < died + u64::from(COMBAT_LINGER_TICKS) {
        session.tick();
    }
    let view = session.zone.snapshot_for_player(1).unwrap();
    assert!(view.viewer.dead && !view.viewer.in_combat);
    assert!(!view.entities[0].flags.in_combat);

    // The walk sent while dead held nothing, so the spirit needs no stop first.
    session.send(ZoneCommand::ReleaseSpirit);
    session.tick();
    let view = session.zone.snapshot_for_player(1).unwrap();
    assert!(!view.viewer.dead);
    assert_eq!(view.viewer.health, 25, "released with half health");
    let position = view.entities[0].position;
    assert_eq!([position[0], position[2]], GRAVEYARD);
    session.send(ZoneCommand::ReleaseSpirit);
    session.tick();
    assert_eq!(
        session.errors().last(),
        Some(&(session.zone.current_tick(), ErrorCode::NotDead))
    );
    // Out of combat, health regenerates 3 % (rounded up) per second after
    // 6 s. The release tick was the first calm tick and the NotDead tick
    // the second, so the first regeneration lands 178 ticks later.
    session.ticks(177);
    assert_eq!(session.health(), 25);
    session.tick();
    assert_eq!(session.health(), 27);
    session.ticks(30);
    assert_eq!(session.health(), 29);
    let position = session.zone.snapshot_for_player(1).unwrap().entities[0].position;
    assert_eq!(
        [position[0], position[2]],
        GRAVEYARD,
        "the released spirit stands"
    );
}

/// A player that dies while running lets go of its held movement: the
/// intent clears at death, `Move` and `Jump` hold nothing while dead, and the
/// released spirit stands at the graveyard until it is told to move again.
/// A checkpoint of the dead player continues exactly.
#[test]
fn held_movement_does_not_survive_death() {
    let content = arena::arena(
        vec![arena::wolf(400, [20, 30])],
        vec![arena::spawn(1, WOLF, [0, 2_000])],
        vec![],
    );
    let mut session = Session::new(Arc::clone(&content));
    session.send(arena::walk(NORTHWARD));
    session.send(ZoneCommand::Jump);
    let died = session.until(600, |session| session.health() == 0);
    let player = &session.zone.snapshot().unwrap().players[0];
    assert_eq!(
        (player.forward, player.strafe, player.jump_pending),
        (0, 0, false),
        "death at tick {died} lets go of the held walk"
    );

    // Accepted and sequenced, but a dead unit holds no movement or jump.
    session.send(ZoneCommand::Move {
        forward: 1,
        strafe: -1,
        facing: WEST,
    });
    session.send(ZoneCommand::Jump);
    let player = &session.zone.snapshot().unwrap().players[0];
    assert_eq!(
        (
            player.forward,
            player.strafe,
            player.jump_pending,
            player.facing
        ),
        (0, 0, false, NORTHWARD)
    );
    assert_eq!(player.last_sequence, session.sequence);

    // Recovery of the dead player reproduces the release and what follows.
    let checkpoint = session.zone.snapshot().unwrap();
    let mut recovered = ZoneSimulation::from_snapshot(checkpoint, content).unwrap();
    session.send(ZoneCommand::ReleaseSpirit);
    recovered
        .apply_command(1, session.sequence, ZoneCommand::ReleaseSpirit)
        .unwrap();
    for _ in 0..90 {
        session.tick();
        recovered.advance_tick().unwrap();
        assert_eq!(
            recovered.snapshot().unwrap(),
            session.zone.snapshot().unwrap()
        );
    }
    let position = session.zone.snapshot_for_player(1).unwrap().entities[0].position;
    assert_eq!(
        [position[0], position[2]],
        GRAVEYARD,
        "the released spirit stands"
    );

    // A fresh move walks again.
    session.send(arena::walk(NORTHWARD));
    session.ticks(10);
    let position = session.zone.snapshot_for_player(1).unwrap().entities[0].position;
    assert_eq!(position[2], GRAVEYARD[1] + 10 * 21);
}

/// A chase beyond 40 m from the spawn point makes the creature evade; it
/// ignores attacks while evading and resets on arrival.
#[test]
fn leashed_creatures_evade_and_ignore_damage() {
    let content = arena::arena(
        vec![arena::wolf(60, [1, 1])],
        vec![arena::spawn(1, WOLF, [0, 2_000])],
        vec![],
    );
    let mut session = Session::new(content);
    let wolf = EntityRef::Creature(FIRST);
    session.send(arena::walk(NORTHWARD));
    session.until(100, |session| {
        session.creature(FIRST).ai == CreatureAi::Engaged
    });
    session.send(ZoneCommand::SelectTarget(Some(wolf)));
    session.send(ZoneCommand::StartAttack);
    // Run south; players outrun creatures (21 vs 19 units per tick).
    session.send(arena::walk(32_768));
    session.until(600, |session| {
        matches!(session.creature(FIRST).ai, CreatureAi::Evading { .. })
    });
    // The leash broke beyond 40 m; the same tick already took one step home.
    let [_, _, z] = session.creature(FIRST).position;
    assert!(
        2_000 - z > 4_000 - CREATURE_EVADE_SPEED_UNITS_PER_TICK,
        "the leash broke 40 m from the spawn (z = {z})"
    );
    assert!(
        session
            .errors()
            .iter()
            .any(|(_, code)| *code == ErrorCode::OutOfRange),
        "attacking while fleeing is out of range"
    );
    let throttled = session.errors();
    assert!(
        throttled.windows(2).all(|pair| pair[1].0 - pair[0].0 >= 30),
        "out-of-range errors are throttled: {throttled:?}"
    );
    // A player next to the evading wolf, with a swing ready, cannot hurt it.
    let mut checkpoint = session.zone.snapshot().unwrap();
    let evading = checkpoint.creatures[0].clone();
    let player = &mut checkpoint.players[0];
    player.position = [evading.position[0], 90, evading.position[2] - 150];
    player.velocity = [0; 3];
    player.forward = 0;
    player.combat.swing_timer = 0;
    assert!(player.combat.auto_attack && player.combat.target == Some(wolf));
    let content = Arc::clone(session.zone.content());
    session.zone = ZoneSimulation::from_snapshot(checkpoint, content).unwrap();
    session.tick();
    assert!(
        session.events.last().is_some_and(|(_, event)| *event
            == ZoneEvent::Evade {
                source: PLAYER,
                target: wolf
            }),
        "{:?}",
        session.events.last()
    );
    let flags = session
        .zone
        .snapshot_for_player(1)
        .unwrap()
        .entities
        .into_iter()
        .find(|entity| entity.entity() == wolf)
        .unwrap()
        .flags;
    assert!(flags.evading && !flags.in_combat && !flags.targets_viewer);
    assert_eq!(session.creature(FIRST).health, evading.health, "no damage");
    session.send(ZoneCommand::StopAttack);
    session.until(600, |session| {
        matches!(session.creature(FIRST).ai, CreatureAi::Idle { .. })
    });
    assert_eq!(session.creature(FIRST).health, 60, "evading resets health");
    assert_eq!(session.creature(FIRST).tapped_by, None, "and the tap");
}

/// However it approaches, an engaged creature closes in to melee reach and
/// swings: integer steering must not stall it just outside its reach while
/// the player, whose reach is longer, keeps hitting it.
#[test]
fn chasing_creatures_close_in_to_melee_from_every_direction() {
    for heading in (0..16_u16).map(|step| step * 4_096 + 1_000) {
        for distance in [400, 555, 650, 777, 900] {
            let (x, z) = mmorpg_core::trig::direction(heading);
            let scale = |unit| mmorpg_core::trig::checked_scale(distance, unit).unwrap();
            let position = [scale(x), scale(z)];
            // Player spawn slots fill the +X/+Z quadrant from the origin.
            if position.iter().all(|&axis| axis > -100) {
                continue;
            }
            let content = arena::arena(
                vec![arena::wolf(30, [1, 2])],
                vec![arena::spawn(1, WOLF, position)],
                vec![],
            );
            let mut session = Session::new(content);
            session.ticks(90);
            let swung = session.events.iter().any(|(_, event)| {
                matches!(
                    event,
                    ZoneEvent::DamageTaken { target: PLAYER, .. }
                        | ZoneEvent::Miss { target: PLAYER, .. }
                )
            });
            let wolf = session.creature(FIRST);
            assert!(
                swung,
                "the wolf from heading {heading}, {distance} units away, stalled at {:?}",
                wolf.position
            );
        }
    }
}

/// Neutral creatures ignore nearby players until attacked; an aggressive
/// creature's radius shrinks for higher-level players.
#[test]
fn neutral_creatures_fight_back_only_when_attacked() {
    let content = arena::arena(
        vec![arena::wolf(30, [1, 2]), arena::boar()],
        vec![arena::spawn(1, BOAR, [-400, 0])],
        vec![],
    );
    let mut session = Session::new(content);
    session.ticks(300);
    assert!(
        matches!(session.creature(FIRST).ai, CreatureAi::Idle { .. }),
        "a neutral boar 4 m away ignores the player"
    );
    session.send(ZoneCommand::SelectTarget(Some(EntityRef::Creature(FIRST))));
    session.send(ZoneCommand::StartAttack);
    session.send(arena::walk(WEST));
    session.until(200, |session| {
        session.creature(FIRST).ai == CreatureAi::Engaged
    });
    let attacked = session.events.iter().any(|(_, event)| {
        matches!(
            event,
            ZoneEvent::DamageDealt { .. } | ZoneEvent::Miss { source: PLAYER, .. }
        )
    });
    assert!(attacked, "the boar engaged because it was attacked");
}

#[test]
fn refused_intents_are_events_not_errors() {
    let content = arena::arena(
        vec![arena::wolf(30, [1, 2])],
        vec![arena::spawn(1, WOLF, [0, 8_000])],
        vec![arena::npc(3, [-300, 300])],
    );
    let mut session = Session::new(content);
    session.send(ZoneCommand::StartAttack);
    session.send(ZoneCommand::SelectTarget(Some(EntityRef::Npc(NpcId::new(
        3,
    )))));
    session.send(ZoneCommand::StartAttack);
    session.send(ZoneCommand::SelectTarget(Some(PLAYER)));
    session.send(ZoneCommand::StartAttack);
    // Beyond the interest radius, unknown or not yet present.
    session.send(ZoneCommand::SelectTarget(Some(EntityRef::Creature(FIRST))));
    session.send(ZoneCommand::SelectTarget(Some(EntityRef::Creature(
        CreatureId::new(9),
    ))));
    session.send(ZoneCommand::SelectTarget(Some(EntityRef::Player(2))));
    session.send(ZoneCommand::ReleaseSpirit);
    session.tick();
    assert_eq!(
        session.errors(),
        [
            (1, ErrorCode::NoTarget),
            (1, ErrorCode::NotAttackable),
            (1, ErrorCode::NotAttackable),
            (1, ErrorCode::InvalidTarget),
            (1, ErrorCode::InvalidTarget),
            (1, ErrorCode::InvalidTarget),
            (1, ErrorCode::NotDead),
        ]
    );
    let view = session.zone.snapshot_for_player(1).unwrap();
    assert_eq!(
        view.viewer.target,
        Some(PLAYER),
        "refused selections keep the target"
    );
    assert_eq!(view.events.len(), 7, "events stay in state for one tick");
    session.tick();
    assert!(
        session
            .zone
            .snapshot_for_player(1)
            .unwrap()
            .events
            .is_empty()
    );
    let npc = session
        .zone
        .snapshot_for_player(1)
        .unwrap()
        .entities
        .into_iter()
        .find(|entity| entity.kind == EntityKind::Npc)
        .unwrap();
    assert_eq!(
        (npc.appearance, npc.level, npc.health_percent),
        (3, 10, 100)
    );
    assert!(!npc.flags.hostile && !npc.flags.attackable);
}

/// Same inputs, same canonical state: two independent runs of a fight agree
/// tick by tick.
#[test]
fn identical_inputs_produce_identical_canonical_state() {
    let run = || {
        let content = arena::arena(
            vec![arena::wolf(80, [20, 30])],
            vec![
                arena::spawn(1, WOLF, [0, 2_000]),
                arena::spawn(2, WOLF, [600, 2_300]),
            ],
            vec![],
        );
        let mut session = Session::new(content);
        session.send(arena::walk(NORTHWARD));
        let mut states = Vec::new();
        for tick in 0..900 {
            if tick == 60 {
                session.send(ZoneCommand::SelectTarget(Some(EntityRef::Creature(FIRST))));
                session.send(ZoneCommand::StartAttack);
            }
            if tick == 400 {
                session.send(ZoneCommand::ReleaseSpirit);
            }
            session.tick();
            states.push(session.zone.snapshot().unwrap());
        }
        states
    };
    assert_eq!(run(), run());
}
