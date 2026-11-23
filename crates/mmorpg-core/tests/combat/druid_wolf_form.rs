//! The Druid's wolf form (#957) in the field-marauder fight of #934 at Millbrook Farm: Entangling
//! Roots holds one marauder, the Druid takes the wolf's shape, closes on another 40% faster and
//! lands Savage Bite, finds Wrath refused in the shape, then returns to caster form for Wrath.
//! The whole sequence replays exactly.
use crate::support;

use std::sync::Arc;

use mmorpg_core::ability::ids;
use mmorpg_core::{
    AbilityId, AuraKind, CanonicalCreatureSnapshot, CanonicalZoneSnapshot, CreatureId, EntityRef,
    ErrorCode, TICK_HZ, ZoneCommand, ZoneEvent, ZoneId, ZoneSimulation, greyhaven_vale,
};

/// The rooted marauder and the one the wolf bites.
const ROOTED: CreatureId = CreatureId::new(221);
const BITTEN: CreatureId = CreatureId::new(224);
/// Open ground south of the fields, beyond every marauder's aggro radius (#934).
const START: [i32; 2] = [7_000, 4_900];
const DRUID: EntityRef = EntityRef::Player(1);

struct Fight {
    zone: ZoneSimulation,
    sequence: u32,
    events: Vec<(u64, ZoneEvent)>,
}

impl Fight {
    /// A level-4 Druid standing at `START`.
    fn new() -> Self {
        let mut zone =
            ZoneSimulation::with_content(ZoneId::new(1), greyhaven_vale::content()).unwrap();
        zone.add_player(1).unwrap();
        let zone = support::classes::at_level(zone, 1, 4);
        let mut state = zone.snapshot().unwrap();
        let ground = zone.content().definition().ground_at(START[0], START[1]);
        state.players[0].position =
            mmorpg_core::physics::from_units([START[0], ground + 100, START[1]]);
        let zone = ZoneSimulation::from_snapshot(state, Arc::clone(zone.content())).unwrap();
        let mut fight = Self {
            zone,
            sequence: 0,
            events: Vec::new(),
        };
        fight.send(ZoneCommand::ChooseClass { class: 6, sex: 1 });
        fight.tick();
        fight
    }

    fn send(&mut self, command: ZoneCommand) {
        self.sequence += 1;
        self.zone.apply_command(1, self.sequence, command).unwrap();
    }

    fn tick(&mut self) {
        self.zone.advance_tick().unwrap();
        let tick = self.zone.current_tick();
        let projection = self.zone.snapshot_for_player(1).unwrap();
        self.events
            .extend(projection.events.into_iter().map(|event| (tick, event)));
    }

    fn ticks(&mut self, count: u64) {
        for _ in 0..count {
            self.tick();
        }
    }

    /// Uses `ability`, facing the creature it is aimed at.
    fn cast(&mut self, ability: AbilityId, target: Option<CreatureId>) {
        let facing = target.map(|id| self.heading_to(id));
        self.send(ZoneCommand::UseAbility {
            ability: ability.get(),
            target: target.map(EntityRef::Creature),
            facing,
        });
        self.tick();
    }

    /// Runs toward creature `id` for `ticks` ticks, then stops; returns the ground covered in
    /// the last `measured` of them.
    fn run_at(&mut self, id: CreatureId, ticks: u64, measured: u64) -> f32 {
        let mut from = xz(self.state().players[0].position);
        for tick in 0..ticks {
            if tick == ticks - measured {
                from = xz(self.state().players[0].position);
            }
            let facing = self.heading_to(id);
            self.send(ZoneCommand::Move {
                forward: 1,
                strafe: 0,
                facing,
            });
            self.tick();
        }
        let covered = distance(from, xz(self.state().players[0].position));
        self.stop();
        covered
    }

    fn stop(&mut self) {
        let facing = self.state().players[0].facing;
        self.send(ZoneCommand::Move {
            forward: 0,
            strafe: 0,
            facing,
        });
        self.tick();
    }

    /// The heading from the Druid toward creature `id`: 0 faces +Z, 16 384 faces +X.
    fn heading_to(&self, id: CreatureId) -> u16 {
        let state = self.state();
        let [x, _, z] = state.players[0].position;
        let [tx, _, tz] = creature(&state, id).position;
        let turn = (tx - x).atan2(tz - z).rem_euclid(std::f32::consts::TAU);
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let heading = (turn / std::f32::consts::TAU * 65_536.0) as u32 as u16;
        heading
    }

    fn state(&self) -> CanonicalZoneSnapshot {
        self.zone.snapshot().unwrap()
    }

    /// Whether the Druid wears the wolf's shape, as its own projection and its record show.
    fn shifted(&self) -> bool {
        let projection = self.zone.snapshot_for_player(1).unwrap();
        let aura = projection
            .auras
            .iter()
            .any(|aura| aura.ability == ids::WOLF_FORM && aura.kind == AuraKind::Shapeshift);
        let drawn = projection
            .entities
            .iter()
            .find(|entity| entity.entity() == DRUID)
            .unwrap()
            .flags
            .shapeshifted;
        assert_eq!(aura, drawn, "the record shows the shape the auras hold");
        aura
    }

    /// The error events of the last tick.
    fn refused(&self) -> Vec<ErrorCode> {
        let now = self.zone.current_tick();
        self.events
            .iter()
            .filter_map(|(tick, event)| match event {
                ZoneEvent::Error { code, .. } if *tick == now => Some(*code),
                _ => None,
            })
            .collect()
    }
}

fn creature(state: &CanonicalZoneSnapshot, id: CreatureId) -> &CanonicalCreatureSnapshot {
    state
        .creatures
        .iter()
        .find(|creature| creature.creature_id == id)
        .unwrap()
}

fn xz(position: [f32; 3]) -> [f32; 2] {
    [position[0], position[2]]
}

fn distance(a: [f32; 2], b: [f32; 2]) -> f32 {
    (a[0] - b[0]).hypot(a[1] - b[1])
}

/// The whole sequence, asserting each beat.
fn journey() -> Fight {
    let second = u64::from(TICK_HZ);
    let mut fight = Fight::new();
    // Savage Bite needs the wolf's shape.
    fight.cast(ids::SAVAGE_BITE, Some(BITTEN));
    assert_eq!(fight.refused(), [ErrorCode::WrongForm]);

    // Entangling Roots holds one marauder where it stands.
    fight.cast(ids::ENTANGLING_ROOTS, Some(ROOTED));
    let state = fight.state();
    assert!(
        creature(&state, ROOTED)
            .abilities
            .auras
            .iter()
            .any(|aura| aura.ability == ids::ENTANGLING_ROOTS)
    );
    fight.ticks(second * 3 / 2);

    // In caster form the Druid runs at the player's pace.
    let walked = fight.run_at(BITTEN, second / 2, second / 4);

    // The wolf's shape: 40% faster over the same quarter second.
    fight.cast(ids::WOLF_FORM, None);
    assert!(fight.shifted(), "the Druid is the wolf");
    let ran = fight.run_at(BITTEN, second / 2, second / 4);
    let ratio = ran / walked;
    assert!(
        (1.35..1.45).contains(&ratio),
        "the wolf runs 40% faster: {walked} then {ran}"
    );

    // Spells are refused in the shape, unpaid.
    fight.ticks(second * 3 / 2);
    fight.cast(ids::WRATH, Some(BITTEN));
    assert_eq!(fight.refused(), [ErrorCode::WrongForm]);
    fight.cast(ids::ENTANGLING_ROOTS, Some(BITTEN));
    assert_eq!(fight.refused(), [ErrorCode::WrongForm]);

    // Close on the second marauder and bite it.
    let mut closed = false;
    for _ in 0..second * 6 {
        let state = fight.state();
        if distance(
            xz(state.players[0].position),
            xz(creature(&state, BITTEN).position),
        ) < 180.0
        {
            closed = true;
            break;
        }
        fight.run_at(BITTEN, 1, 1);
    }
    assert!(closed, "the wolf reaches the marauder");
    let health = creature(&fight.state(), BITTEN).health;
    fight.cast(ids::SAVAGE_BITE, Some(BITTEN));
    assert!(
        fight.refused().is_empty(),
        "the bite lands: {:?}",
        fight.refused()
    );
    fight.ticks(second / 2);
    assert!(
        creature(&fight.state(), BITTEN).health < health,
        "Savage Bite wounds the marauder"
    );

    // Wolf Form again returns the Druid to caster form, where Wrath casts once more.
    fight.ticks(second * 3 / 2);
    fight.cast(ids::WOLF_FORM, None);
    assert!(!fight.shifted(), "the Druid is back in caster form");
    assert!(fight.events.contains(&(
        fight.zone.current_tick(),
        ZoneEvent::AuraRemoved {
            source: DRUID,
            target: DRUID,
            ability: ids::WOLF_FORM,
        }
    )));
    fight.ticks(second * 3 / 2);
    fight.cast(ids::WRATH, Some(BITTEN));
    assert!(
        fight.refused().is_empty(),
        "Wrath casts: {:?}",
        fight.refused()
    );
    assert!(fight.events.iter().any(|(_, event)| matches!(
        event,
        ZoneEvent::CastStarted {
            source: DRUID,
            ability: ids::WRATH,
            ..
        }
    )));
    fight
}

#[test]
fn the_druid_shifts_bites_in_wolf_form_and_shifts_back_for_spells() {
    let first = journey();
    // The same commands replay to the same zone and the same events.
    let second = journey();
    assert_eq!(first.state(), second.state());
    assert_eq!(first.events, second.events);
}
