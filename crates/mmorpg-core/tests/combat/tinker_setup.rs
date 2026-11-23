//! The Tinker's reference encounter (#950), Stage 1 geometry with today's thrown weapons: the
//! existing Rogue Venom Flask and Blasting Bomb (#290) stand in for the Tinker's flask and bomb
//! against the Rimewood scouts (#933). A Rogue 14.5 m east of the side-by-side pair lobs the flask
//! onto them, both are poisoned and the lookout 7 m off is not; the pair charge into reach, and the
//! bomb bursts on them while the lookout is still beyond its 4 m, so only the pair are hit. No
//! chemical payoff exists yet; this pins the inclusion and exclusion the prototype builds on
//! (`docs/STARTER_ZONE.md`, Tinker reference encounter). The same commands replay exactly.
use crate::support;

use mmorpg_core::ability::ids;
use mmorpg_core::greyhaven_vale::{self, rimewood};
use mmorpg_core::{
    CanonicalZoneSnapshot, CreatureId, EntityRef, Flight, ProjectileKind, ZoneCommand, ZoneEvent,
    ZoneId, ZoneSimulation,
};
use support::{arena, classes};

const PLAYER: u32 = 1;
const ROGUE: u8 = 4;
const WEST: u16 = 49_152;
/// The flask's splash and the bomb's blast radii (ability catalog 43 and 44), units.
const FLASK_RADIUS: f32 = 300.0;
const BOMB_RADIUS: f32 = 400.0;

/// A level 6 Rogue 14.5 m east of the pair, facing them.
fn start() -> ZoneSimulation {
    let content = greyhaven_vale::content();
    let mut zone = ZoneSimulation::with_content(ZoneId::new(1), content).unwrap();
    zone.add_player(PLAYER).unwrap();
    let zone = classes::at_level(zone, PLAYER, 6);
    let mut state = zone.snapshot().unwrap();
    let [x, z] = rimewood::PAIR_AT;
    let x = x + 1_450;
    let ground = zone.content().definition().ground_at(x, z);
    state.players[0].position = mmorpg_core::physics::from_units([x, ground + 100, z]);
    ZoneSimulation::from_snapshot(state, std::sync::Arc::clone(zone.content())).unwrap()
}

struct Journey {
    zone: ZoneSimulation,
    commands: Vec<(usize, ZoneCommand)>,
    events: Vec<Vec<ZoneEvent>>,
    sequence: u32,
}

impl Journey {
    fn new() -> Self {
        Self {
            zone: start(),
            commands: Vec::new(),
            events: Vec::new(),
            sequence: 0,
        }
    }

    fn tick(&mut self, command: Option<ZoneCommand>) -> &[ZoneEvent] {
        if let Some(command) = command {
            self.sequence += 1;
            self.commands.push((self.events.len(), command));
            self.zone
                .apply_command(PLAYER, self.sequence, command)
                .unwrap();
        }
        self.zone.advance_tick().unwrap();
        let events = self.zone.snapshot_for_player(PLAYER).unwrap().events;
        self.events.push(events);
        self.events.last().unwrap()
    }

    fn state(&self) -> CanonicalZoneSnapshot {
        self.zone.snapshot().unwrap()
    }

    /// Ticks until the global cooldown is over.
    fn ready(&mut self) {
        for _ in 0..120 {
            if self.state().players[0].combat.abilities.global_cooldown == 0 {
                return;
            }
            self.tick(None);
        }
        panic!("the global cooldown never ends");
    }

    /// Throws `ability` at `at` (selected and faced, as a player aims) and runs until its
    /// projectile bursts; returns where.
    fn throw(
        &mut self,
        ability: mmorpg_core::AbilityId,
        kind: ProjectileKind,
        at: CreatureId,
    ) -> [f32; 3] {
        let unit = EntityRef::Creature(at);
        let (facing, _) = arena::aim_at(&self.zone, PLAYER, unit).unwrap();
        self.tick(Some(ZoneCommand::UseAbility {
            ability: ability.get(),
            target: Some(unit),
            facing: Some(facing),
        }));
        for _ in 0..240 {
            let burst = self.state().projectiles.iter().find_map(|p| {
                (p.kind == kind && matches!(p.flight, Flight::Burst { .. })).then_some(p.position)
            });
            if let Some(at) = burst {
                return at;
            }
            self.tick(None);
        }
        panic!("the {kind:?} never bursts");
    }
}

fn creature_at(state: &CanonicalZoneSnapshot, id: CreatureId) -> [f32; 3] {
    state
        .creatures
        .iter()
        .find(|c| c.creature_id == id)
        .unwrap()
        .position
}

fn apart(a: [f32; 3], b: [f32; 3]) -> f32 {
    (a[0] - b[0]).hypot(a[2] - b[2])
}

fn poisoned(state: &CanonicalZoneSnapshot, id: CreatureId) -> bool {
    state
        .creatures
        .iter()
        .find(|c| c.creature_id == id)
        .is_some_and(|c| {
            c.abilities
                .auras
                .iter()
                .any(|a| a.ability == ids::VENOM_FLASK)
        })
}

fn damaged(events: &[Vec<ZoneEvent>], id: CreatureId) -> bool {
    events.iter().flatten().any(|event| {
        matches!(*event, ZoneEvent::DamageDealt { target, amount, .. }
            if target == EntityRef::Creature(id) && amount > 0)
    })
}

fn journey() -> Journey {
    let mut j = Journey::new();
    let [first, second] = rimewood::PAIR;
    let lookout = rimewood::LOOKOUT;
    j.tick(Some(ZoneCommand::ChooseClass {
        class: ROGUE,
        sex: 0,
    }));
    j.tick(Some(arena::stand(WEST)));

    // Combine: the flask shatters on the pair; both are inside its 3 m, the lookout is not.
    let flask = j.throw(ids::VENOM_FLASK, ProjectileKind::Flask, first);
    let state = j.state();
    for id in [first, second] {
        assert!(apart(flask, creature_at(&state, id)) < FLASK_RADIUS);
    }
    assert!(apart(flask, creature_at(&state, lookout)) > FLASK_RADIUS);
    assert!(poisoned(&state, first) && poisoned(&state, second));
    assert!(!poisoned(&state, lookout));

    // Attract: the poisoned pair charge together and stand in reach, the lookout trailing. A
    // bomb lobbed at them while they run lands behind them (they run under its arc), which is
    // what the Tinker's trap is for; standing still, they are caught.
    j.ready();
    for _ in 0..600 {
        let s = j.state();
        let me = s.players[0].position;
        let near = |id| apart(me, creature_at(&s, id)) < 300.;
        if near(first) && near(second) {
            break;
        }
        j.tick(None);
    }

    // Detonate: the bomb bursts on the pair while the lookout is still beyond its 4 m.
    let before = j.events.len();
    let bomb = j.throw(ids::BLASTING_BOMB, ProjectileKind::Bomb, first);
    // The blast resolves on the burst tick; give its events a tick to project.
    j.tick(None);
    let state = j.state();
    // Only the burst tick and the next: the flask's poison pulses are not the blast.
    let blast = &j.events[j.events.len() - 2..];
    assert!(j.events.len() - before >= 2);
    assert!(
        apart(bomb, creature_at(&state, lookout)) > BOMB_RADIUS,
        "the lookout stays outside the blast"
    );
    assert!(damaged(blast, first) && damaged(blast, second));
    // The lookout was never hit: not by the flask, not by the bomb.
    assert!(!damaged(&j.events, lookout));
    j
}

#[test]
fn flask_then_bomb_catches_the_rimewood_pair_and_spares_the_lookout_and_replays_exactly() {
    let played = journey();
    let mut replay = Journey::new();
    let mut commands = played.commands.iter().peekable();
    for tick in 0..played.events.len() {
        let command = commands
            .next_if(|(at, _)| *at == tick)
            .map(|&(_, command)| command);
        replay.tick(command);
    }
    assert_eq!(replay.events, played.events);
    assert_eq!(replay.state(), played.state());
}
