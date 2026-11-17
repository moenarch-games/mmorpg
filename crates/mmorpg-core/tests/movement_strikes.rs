//! Movement-state melee (#885, Atlas 5.2.i885): the basic attack's standing chain, running
//! and jumping strikes, their contact ticks, lunge and landing blow, the weapon families and
//! recovery mid-chain.
mod support;

use std::sync::Arc;

use mmorpg_core::{
    CHAIN_WINDOW_TICKS, CreatureBehaviour, CreatureTemplate, EntityRef, FamilyMelee, StrikeVariant,
    WeaponFamily, ZoneAreas, ZoneCommand, ZoneContent, ZoneEvent, ZoneId, ZoneSimulation,
};
use support::arena::{self, GRAVEYARD, WOLF};

const SOUTH: u16 = 32_768;
const NORTH: u16 = 0;
const SWORD: FamilyMelee = FamilyMelee::of(WeaponFamily::Sword);

/// A sturdy neutral creature 1.5 m south of the spawn at the origin.
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
            vec![arena::spawn(1, WOLF, [0, -150])],
            Vec::new(),
            GRAVEYARD,
        )
        .unwrap()
        .with_creature_abilities(Default::default())
        .unwrap(),
    )
}

struct Striker {
    zone: ZoneSimulation,
    sequence: u32,
    ticks: u32,
}

/// A strike that began, and the tick it began on.
#[derive(Debug)]
struct Began {
    tick: u32,
    family: WeaponFamily,
    variant: StrikeVariant,
    contact: u16,
}

impl Striker {
    /// A player of `class` (0 the Warden) facing `facing`.
    fn new(class: u8, facing: u16) -> Self {
        let mut zone = ZoneSimulation::with_content(ZoneId::new(9), content()).unwrap();
        zone.add_player(1).unwrap();
        let mut striker = Self {
            zone,
            sequence: 0,
            ticks: 0,
        };
        striker.send(ZoneCommand::ChooseClass { class, sex: 0 });
        striker.send(arena::stand(facing));
        striker.tick();
        striker
    }

    fn send(&mut self, command: ZoneCommand) {
        self.sequence += 1;
        self.zone.apply_command(1, self.sequence, command).unwrap();
    }

    fn tick(&mut self) -> Vec<ZoneEvent> {
        self.ticks += 1;
        self.zone.advance_tick().unwrap();
        self.zone.snapshot_for_player(1).unwrap().events
    }

    /// Ticks until `stop` holds for a tick's events, at most `limit` ticks; every strike that
    /// began on the way and the ticks it took.
    fn run(&mut self, limit: u32, stop: impl Fn(&[ZoneEvent]) -> bool) -> (Vec<Began>, u32) {
        let mut began = Vec::new();
        for tick in 1..=limit {
            let events = self.tick();
            for event in &events {
                if let ZoneEvent::StrikeBegan {
                    family,
                    variant,
                    contact,
                    ..
                } = *event
                {
                    began.push(Began {
                        tick: self.ticks,
                        family,
                        variant,
                        contact,
                    });
                }
            }
            if stop(&events) {
                return (began, tick);
            }
        }
        panic!("nothing happened in {limit} ticks: {began:?}");
    }

    fn position(&self) -> [f32; 3] {
        self.zone.snapshot().unwrap().players[0].position
    }
}

fn contact(events: &[ZoneEvent]) -> bool {
    events.iter().any(|event| {
        matches!(
            event,
            ZoneEvent::AttackPerformed {
                source: EntityRef::Player(1),
                ..
            }
        )
    })
}

fn began(events: &[ZoneEvent]) -> bool {
    events
        .iter()
        .any(|event| matches!(event, ZoneEvent::StrikeBegan { .. }))
}

#[test]
fn a_standing_strike_winds_up_and_hits_on_its_contact_tick() {
    let mut warden = Striker::new(0, SOUTH);
    warden.send(ZoneCommand::Swing { facing: SOUTH });
    let (strikes, _) = warden.run(120, contact);
    let [first] = &strikes[..] else {
        panic!("one strike: {strikes:?}");
    };
    assert_eq!(
        (first.family, first.variant, first.contact),
        (
            WeaponFamily::Sword,
            StrikeVariant::First,
            SWORD.chain[0].contact_ticks
        )
    );
    // Nothing lands before the contact tick, and the blow lands exactly on it.
    assert_eq!(warden.ticks - first.tick, u32::from(first.contact));
}

#[test]
fn pressing_again_in_the_window_chains_first_second_and_finisher() {
    let mut warden = Striker::new(0, SOUTH);
    let mut strikes = Vec::new();
    for _ in 0..4 {
        // Presses while a strike recovers wait for it, so pressing early chains on time.
        warden.send(ZoneCommand::Swing { facing: SOUTH });
        let (mut next, _) = warden.run(240, began);
        strikes.append(&mut next);
    }
    let variants: Vec<_> = strikes.iter().map(|strike| strike.variant).collect();
    assert_eq!(
        variants,
        [
            StrikeVariant::First,
            StrikeVariant::Second,
            StrikeVariant::Finisher,
            StrikeVariant::First
        ]
    );
    // Each follows its predecessor's interval to the tick.
    let gaps: Vec<_> = strikes.windows(2).map(|pair| pair[1].tick).collect();
    let mut at = 0;
    for (index, gap) in gaps.iter().enumerate() {
        at += u32::from(SWORD.chain[index].interval_ticks);
        assert_eq!(*gap - strikes[0].tick, at, "strike {}", index + 1);
    }
}

#[test]
fn a_press_after_the_window_starts_the_chain_over() {
    let mut warden = Striker::new(0, SOUTH);
    warden.send(ZoneCommand::Swing { facing: SOUTH });
    warden.run(120, began);
    let wait = SWORD.chain[0].interval_ticks + CHAIN_WINDOW_TICKS;
    for _ in 0..wait {
        warden.tick();
    }
    warden.send(ZoneCommand::Swing { facing: SOUTH });
    let (strikes, _) = warden.run(120, began);
    assert_eq!(strikes[0].variant, StrikeVariant::First);
}

#[test]
fn a_running_strike_lunges_its_distance_and_ends_the_chain() {
    // Running north, away from the creature, so nothing stops the lunge.
    let mut warden = Striker::new(0, NORTH);
    warden.send(arena::walk(NORTH));
    for _ in 0..20 {
        warden.tick();
    }
    warden.send(ZoneCommand::Swing { facing: NORTH });
    let (strikes, _) = warden.run(10, began);
    assert_eq!(strikes[0].variant, StrikeVariant::Running);
    warden.send(arena::stand(NORTH));
    let from = warden.position();
    // Standing intent no longer stops the body: the lunge carries it to the contact.
    warden.run(120, contact);
    assert_eq!(
        warden.ticks - strikes[0].tick,
        u32::from(SWORD.running.contact_ticks)
    );
    let to = warden.position();
    let travelled = (to[0] - from[0]).hypot(to[2] - from[2]);
    let lunge = SWORD.lunge_units as f32;
    assert!(
        (travelled - lunge).abs() <= 3.,
        "travelled {travelled} of {lunge}"
    );
    let melee = warden.zone.snapshot().unwrap().players[0].combat.melee;
    assert_eq!((melee.chain, melee.pending), (0, None));
}

#[test]
fn a_jumping_strike_lands_its_blow_on_touchdown_then_recovers() {
    let mut warden = Striker::new(0, SOUTH);
    warden.send(ZoneCommand::Jump);
    warden.tick();
    warden.send(ZoneCommand::Swing { facing: SOUTH });
    let (strikes, _) = warden.run(10, began);
    assert_eq!(strikes[0].variant, StrikeVariant::Jumping);
    let mut airborne_ticks = 0;
    loop {
        let events = warden.tick();
        let landing = warden.zone.snapshot().unwrap().players[0].combat.landing;
        if contact(&events) {
            // The blow lands on the tick the body touches down, not in the air.
            assert!(!landing.airborne);
            assert!(landing.recovery > 0, "it lands this tick");
            break;
        }
        assert!(landing.airborne, "no blow before touchdown");
        airborne_ticks += 1;
        assert!(airborne_ticks < 120);
    }
    let combat = warden.zone.snapshot().unwrap().players[0].combat.clone();
    assert_eq!(combat.swing_timer, SWORD.jumping.interval_ticks);
    assert_eq!(combat.melee.chain, 0);
}

#[test]
fn every_class_strikes_with_its_weapon_family() {
    for (class, family) in [
        (0, WeaponFamily::Sword),
        (1, WeaponFamily::Bow),
        (2, WeaponFamily::Staff),
        (4, WeaponFamily::Dagger),
        (5, WeaponFamily::Mace),
    ] {
        let mut striker = Striker::new(class, SOUTH);
        striker.send(ZoneCommand::Swing { facing: SOUTH });
        let (strikes, _) = striker.run(10, began);
        let profile = FamilyMelee::of(family).chain[0];
        assert_eq!(
            (strikes[0].family, strikes[0].contact),
            (family, profile.contact_ticks),
            "class {class}"
        );
    }
    // An equipped weapon picks its own family: the starter bag's Worn Dagger.
    let mut warden = Striker::new(0, SOUTH);
    warden.send(ZoneCommand::EquipItem { bag_slot: 1 });
    warden.tick();
    warden.send(ZoneCommand::Swing { facing: SOUTH });
    let (strikes, _) = warden.run(10, began);
    assert_eq!(strikes[0].family, WeaponFamily::Dagger);
}

#[test]
fn a_zone_restored_mid_chain_continues_exactly() {
    let mut original = Striker::new(0, SOUTH);
    original.send(ZoneCommand::Swing { facing: SOUTH });
    original.run(120, contact);
    // Mid-chain, the second strike pending: press, begin, then restore during its wind-up.
    original.send(ZoneCommand::Swing { facing: SOUTH });
    original.run(120, began);
    original.tick();
    let snapshot = original.zone.snapshot().unwrap();
    let melee = snapshot.players[0].combat.melee;
    assert_eq!(melee.chain, 2);
    assert!(
        melee
            .pending
            .is_some_and(|pending| pending.variant == StrikeVariant::Second)
    );
    let content = Arc::clone(original.zone.content());
    let mut restored = Striker {
        zone: ZoneSimulation::from_snapshot(snapshot, content).unwrap(),
        sequence: original.sequence,
        ticks: original.ticks,
    };
    for _ in 0..20 {
        let expected = original.tick();
        assert_eq!(restored.tick(), expected);
    }
    assert_eq!(
        restored.zone.snapshot().unwrap(),
        original.zone.snapshot().unwrap()
    );
    // The next press continues the restored chain with its finisher.
    for striker in [&mut original, &mut restored] {
        striker.send(ZoneCommand::Swing { facing: SOUTH });
        let (strikes, _) = striker.run(120, began);
        assert_eq!(strikes[0].variant, StrikeVariant::Finisher);
    }
}

#[test]
fn a_jumping_strike_in_a_long_fall_lands_in_the_air() {
    let mut warden = Striker::new(0, SOUTH);
    // 30 m up: the fall outlasts the jumping strike's longest flight.
    let mut state = warden.zone.snapshot().unwrap();
    state.players[0].position[1] += 3_000.0;
    let content = Arc::clone(warden.zone.content());
    warden.zone = ZoneSimulation::from_snapshot(state, content).unwrap();
    warden.tick();
    warden.send(ZoneCommand::Swing { facing: SOUTH });
    let (strikes, _) = warden.run(10, began);
    assert_eq!(strikes[0].variant, StrikeVariant::Jumping);
    warden.run(240, contact);
    assert_eq!(
        warden.ticks - strikes[0].tick,
        u32::from(mmorpg_core::JUMP_STRIKE_AIR_TICKS)
    );
    let landing = warden.zone.snapshot().unwrap().players[0].combat.landing;
    assert!(landing.airborne, "still falling, the weapon is free again");
}
