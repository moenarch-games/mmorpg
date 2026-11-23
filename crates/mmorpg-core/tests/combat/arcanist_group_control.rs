//! An Arcanist's group-control journey against the Rimewood scouts (#933): the pair approach and
//! Frost Nova roots them while the lookout is still outside its 8 m, the Arcanist backs off
//! with the lookout following, and Chain Lightning on a rooted scout jumps to the other and not
//! to the lookout, now beyond 8 m of them. The chain's damage breaks the roots. The same
//! commands replay to the same zone.
use crate::support;

use mmorpg_core::ability::ids;
use mmorpg_core::greyhaven_vale::{self, rimewood};
use mmorpg_core::{
    CanonicalZoneSnapshot, CreatureAi, CreatureId, EntityRef, ZoneCommand, ZoneEvent, ZoneId,
    ZoneSimulation,
};
use support::{arena, classes};

const PLAYER: u32 = 1;
const SOUTH_EAST: u16 = 8_192;
const WEST: u16 = 49_152;
const ARCANIST: u8 = 2;

fn scout(id: CreatureId) -> EntityRef {
    EntityRef::Creature(id)
}

/// A level 7 Arcanist 20 m east of the pair.
fn start() -> ZoneSimulation {
    let content = greyhaven_vale::content();
    let mut zone = ZoneSimulation::with_content(ZoneId::new(1), content).unwrap();
    zone.add_player(PLAYER).unwrap();
    let zone = classes::at_level(zone, PLAYER, 7);
    let mut state = zone.snapshot().unwrap();
    let [x, z] = rimewood::PAIR_AT;
    let x = x + 2_000;
    let ground = zone.content().definition().ground_at(x, z);
    state.players[0].position = mmorpg_core::physics::from_units([x, ground + 100, z]);
    ZoneSimulation::from_snapshot(state, std::sync::Arc::clone(zone.content())).unwrap()
}

/// The zone, every command it was given by tick and every event the player saw.
struct Journey {
    zone: ZoneSimulation,
    commands: Vec<(usize, ZoneCommand)>,
    events: Vec<Vec<ZoneEvent>>,
    sequence: u32,
}

impl Journey {
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

    /// Ticks with `command` first until `done`, at most `limit` ticks.
    fn until(
        &mut self,
        limit: usize,
        mut command: Option<ZoneCommand>,
        done: impl Fn(&CanonicalZoneSnapshot) -> bool,
    ) {
        for _ in 0..limit {
            if done(&self.zone.snapshot().unwrap()) {
                return;
            }
            self.tick(command.take());
        }
        panic!("not reached in {limit} ticks");
    }

    fn state(&self) -> CanonicalZoneSnapshot {
        self.zone.snapshot().unwrap()
    }
}

/// XZ distance between the player and `creature`, units.
fn apart(state: &CanonicalZoneSnapshot, a: [f32; 3], creature: CreatureId) -> f32 {
    let b = creature_at(state, creature);
    (a[0] - b[0]).hypot(a[2] - b[2])
}

fn creature_at(state: &CanonicalZoneSnapshot, id: CreatureId) -> [f32; 3] {
    let creature = state
        .creatures
        .iter()
        .find(|c| c.creature_id == id)
        .unwrap();
    creature.position
}

fn rooted(state: &CanonicalZoneSnapshot, id: CreatureId) -> bool {
    state
        .creatures
        .iter()
        .find(|c| c.creature_id == id)
        .is_some_and(|c| {
            c.abilities
                .auras
                .iter()
                .any(|a| a.ability == ids::FROST_NOVA)
        })
}

fn mana(state: &CanonicalZoneSnapshot) -> u16 {
    state.players[0].combat.abilities.resource
}

fn hit(events: &[ZoneEvent], unit: EntityRef) -> bool {
    events.iter().any(|event| {
        matches!(*event, ZoneEvent::DamageDealt { target, amount, .. } if target == unit && amount > 0)
    })
}

fn freed(events: &[ZoneEvent], unit: EntityRef) -> bool {
    events.iter().any(|event| {
        matches!(*event, ZoneEvent::AuraRemoved { target, ability, .. }
            if target == unit && ability == ids::FROST_NOVA)
    })
}

fn journey() -> Journey {
    let mut j = Journey {
        zone: start(),
        commands: Vec::new(),
        events: Vec::new(),
        sequence: 0,
    };
    let [first, second] = rimewood::PAIR;
    let lookout = rimewood::LOOKOUT;
    j.tick(Some(ZoneCommand::ChooseClass {
        class: ARCANIST,
        sex: 0,
    }));

    // Walk west until the scouts notice, then hold and let them come: the pair arrive together.
    j.until(600, Some(arena::walk(WEST)), |s| {
        s.creatures
            .iter()
            .any(|c| c.creature_id == first && c.ai == CreatureAi::Engaged)
    });
    j.tick(Some(arena::stand(WEST)));
    j.until(600, None, |s| {
        let me = s.players[0].position;
        apart(s, me, first) < 300. && apart(s, me, second) < 300.
    });
    let state = j.state();
    let me = state.players[0].position;
    assert!(
        apart(&state, me, lookout) > 800.,
        "the lookout is still outside the nova"
    );
    for id in [first, second, lookout] {
        assert!(
            state
                .creatures
                .iter()
                .any(|c| c.creature_id == id && c.ai == CreatureAi::Engaged),
            "scout {} joined the fight",
            id.get()
        );
    }

    // Frost Nova roots the pair, not the lookout.
    let before = mana(&state);
    j.tick(Some(ZoneCommand::UseAbility {
        ability: ids::FROST_NOVA.get(),
        target: None,
        facing: None,
    }));
    let state = j.state();
    assert_eq!(before - mana(&state), 30, "Frost Nova's mana");
    assert!(rooted(&state, first) && rooted(&state, second));
    assert!(!rooted(&state, lookout));
    let held = [creature_at(&state, first), creature_at(&state, second)];

    // Back off south-east: the rooted pair stay, the lookout follows past their side.
    j.until(240, Some(arena::walk(SOUTH_EAST)), |s| {
        let me = s.players[0].position;
        apart(s, me, first) > 1_200. && apart(s, me, second) > 1_200.
    });
    j.tick(Some(arena::stand(SOUTH_EAST)));
    let state = j.state();
    assert!(rooted(&state, first) && rooted(&state, second));
    assert_eq!(
        [creature_at(&state, first), creature_at(&state, second)],
        held,
        "the roots hold the pair"
    );

    // Chain Lightning on a rooted scout: it jumps to the other, not to the lookout.
    j.until(120, None, |s| {
        s.players[0].combat.abilities.global_cooldown == 0
    });
    let state = j.state();
    let (facing, _) = arena::aim_at(&j.zone, PLAYER, scout(first)).unwrap();
    let before = mana(&state);
    j.tick(Some(ZoneCommand::UseAbility {
        ability: ids::CHAIN_LIGHTNING.get(),
        target: Some(scout(first)),
        facing: Some(facing),
    }));
    let mut landed = None;
    for _ in 0..180 {
        let events = j.tick(None);
        if events.iter().any(|event| {
            matches!(*event, ZoneEvent::AbilityUsed { ability, .. } if ability == ids::CHAIN_LIGHTNING)
        }) {
            landed = Some(events.to_vec());
            break;
        }
    }
    let events = landed.expect("Chain Lightning lands");
    let state = j.state();
    assert_eq!(before - mana(&state), 45, "Chain Lightning's mana");
    let lookout_from_pair = [first, second].map(|id| {
        let at = creature_at(&state, id);
        apart(&state, at, lookout)
    });
    assert!(
        lookout_from_pair.iter().all(|&d| d > 800.),
        "the lookout is beyond a jump: {lookout_from_pair:?}"
    );
    assert!(hit(&events, scout(first)) && hit(&events, scout(second)));
    assert!(!hit(&events, scout(lookout)));
    // The chain's damage breaks both roots, their unbreakable second long past.
    assert!(freed(&events, scout(first)) && freed(&events, scout(second)));
    assert!(!rooted(&state, first) && !rooted(&state, second));
    j
}

#[test]
fn frost_nova_then_chain_lightning_controls_the_rimewood_scouts_and_replays_exactly() {
    let played = journey();
    // The same commands on the same ticks give the same zone and the same events.
    let mut replay = Journey {
        zone: start(),
        commands: Vec::new(),
        events: Vec::new(),
        sequence: 0,
    };
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
