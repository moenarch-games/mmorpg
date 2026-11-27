//! Movement-state melee (#885, Atlas 5.2.i885): the basic attack's standing chain, running
//! and jumping strikes, their contact ticks, lunge and landing blow, the weapon families and
//! recovery mid-chain, and their timing rewards (#928): the landing strike and the follow-up.
use crate::support;

use std::sync::Arc;

use mmorpg_core::{
    CHAIN_WINDOW_TICKS, CreatureBehaviour, CreatureTemplate, EntityRef, FOLLOW_UP_WINDOW_TICKS,
    FamilyMelee, LANDING_STRIKE_TOLERANCE_TICKS, StrikeReward, StrikeVariant, WeaponFamily,
    ZoneAreas, ZoneCommand, ZoneContent, ZoneEvent, ZoneId, ZoneSimulation,
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

fn rewarded(events: &[ZoneEvent], reward: StrikeReward) -> bool {
    events.iter().any(|event| {
        matches!(
            *event,
            ZoneEvent::StrikeRewarded { source: EntityRef::Player(1), reward: r, .. } if r == reward
        )
    })
}

#[test]
fn a_jumping_strike_timed_with_its_landing_halves_its_recovery() {
    // The jump's touchdown tick, counted from the jump command.
    let mut probe = Striker::new(0, SOUTH);
    probe.send(ZoneCommand::Jump);
    let mut touchdown = 0;
    loop {
        probe.tick();
        touchdown += 1;
        let landing = probe.zone.snapshot().unwrap().players[0].combat.landing;
        if touchdown > 1 && !landing.airborne {
            break;
        }
        assert!(touchdown < 240, "the jump lands");
    }
    let windup = u32::from(SWORD.jumping.contact_ticks);
    let tolerance = u32::from(LANDING_STRIKE_TOLERANCE_TICKS);
    assert!(
        touchdown > windup + tolerance + 2,
        "the jump outlasts the wind-up"
    );
    let (mut timed, mut early, mut late) = (0, 0, 0);
    // Press on every tick of the flight: early, on time and late against the landing.
    for delay in 1..touchdown - 1 {
        let mut warden = Striker::new(0, SOUTH);
        let jumped = warden.ticks;
        warden.send(ZoneCommand::Jump);
        for _ in 0..delay {
            warden.tick();
        }
        warden.send(ZoneCommand::Swing { facing: SOUTH });
        let (strikes, _) = warden.run(10, began);
        assert_eq!(strikes[0].variant, StrikeVariant::Jumping);
        let windup_end = strikes[0].tick + windup;
        let mut events = Vec::new();
        while !contact(&events) {
            events = warden.tick();
        }
        let landed = jumped + touchdown;
        let on_time = landed.abs_diff(windup_end) <= tolerance;
        let swing_timer = warden.zone.snapshot().unwrap().players[0]
            .combat
            .swing_timer;
        let reward = rewarded(&events, StrikeReward::LandingStrike);
        assert_eq!(reward, on_time, "press {delay} ticks into the jump");
        if on_time {
            timed += 1;
            assert_eq!(swing_timer, SWORD.jumping.interval_ticks / 2);
        } else {
            // A mistimed landing recovers in full, as before.
            assert_eq!(swing_timer, SWORD.jumping.interval_ticks);
            if landed < windup_end {
                early += 1;
            } else {
                late += 1;
            }
        }
    }
    assert!(timed > 0 && early > 0 && late > 0, "{timed} {early} {late}");
}

#[test]
fn a_press_inside_the_follow_up_window_swings_at_once() {
    let mut warden = Striker::new(0, SOUTH);
    warden.send(ZoneCommand::Swing { facing: SOUTH });
    let (strikes, _) = warden.run(120, contact);
    let first = &strikes[0];
    let contact_tick = warden.ticks;
    // The hit on the creature ahead opened the window, shown to the striker.
    let melee = warden.zone.snapshot().unwrap().players[0].combat.melee;
    assert_eq!(melee.follow_up, FOLLOW_UP_WINDOW_TICKS);
    let quick = warden.zone.snapshot_for_player(1).unwrap().events;
    assert!(rewarded(&quick, StrikeReward::FollowUp));
    for _ in 0..4 {
        warden.tick();
    }
    warden.send(ZoneCommand::Swing { facing: SOUTH });
    let (next, _) = warden.run(2, began);
    assert_eq!(next[0].variant, StrikeVariant::Second);
    assert_eq!(next[0].tick, contact_tick + 5);
    assert!(next[0].tick - first.tick < u32::from(SWORD.chain[0].interval_ticks));
}

#[test]
fn a_press_after_the_follow_up_window_waits_out_the_recovery() {
    let mut warden = Striker::new(0, SOUTH);
    warden.send(ZoneCommand::Swing { facing: SOUTH });
    let (strikes, _) = warden.run(120, contact);
    let first = &strikes[0];
    for _ in 0..FOLLOW_UP_WINDOW_TICKS {
        warden.tick();
    }
    let combat = warden.zone.snapshot().unwrap().players[0].combat.clone();
    assert_eq!(combat.melee.follow_up, 0);
    assert!(combat.swing_timer > 0, "still recovering");
    warden.send(ZoneCommand::Swing { facing: SOUTH });
    let (next, _) = warden.run(120, began);
    assert_eq!(next[0].variant, StrikeVariant::Second);
    assert_eq!(
        next[0].tick - first.tick,
        u32::from(SWORD.chain[0].interval_ticks)
    );
}

#[test]
fn a_whiff_or_a_press_already_waiting_opens_no_follow_up() {
    // Facing away from the creature, the strike covers nobody.
    let mut whiff = Striker::new(0, NORTH);
    whiff.send(ZoneCommand::Swing { facing: NORTH });
    whiff.run(120, contact);
    let events = whiff.zone.snapshot_for_player(1).unwrap().events;
    assert!(!rewarded(&events, StrikeReward::FollowUp));
    assert_eq!(
        whiff.zone.snapshot().unwrap().players[0]
            .combat
            .melee
            .follow_up,
        0
    );
    // A press during the wind-up keeps the ordinary chain after the recovery.
    let mut eager = Striker::new(0, SOUTH);
    eager.send(ZoneCommand::Swing { facing: SOUTH });
    eager.run(2, began);
    eager.send(ZoneCommand::Swing { facing: SOUTH });
    eager.run(120, contact);
    assert_eq!(
        eager.zone.snapshot().unwrap().players[0]
            .combat
            .melee
            .follow_up,
        0
    );
}

/// The basic strike as the action graph reports it (#1221).
mod graph {
    use super::*;
    use mmorpg_core::action::{
        self, ActionGraph, ActionId, Consequence, Interrupted, InterruptionRule, Movement,
        MovementRule, Phase, Region, RegionReport,
    };
    use mmorpg_core::{Aura, ability::ids};

    impl Striker {
        fn offense(&self) -> RegionReport {
            self.zone.action_inspection(1).unwrap().regions[Region::Offense as usize]
        }

        fn combat(&self) -> mmorpg_core::CanonicalPlayerCombat {
            self.zone.snapshot().unwrap().players[0].combat.clone()
        }

        /// Restores the zone with a stun of `ticks` from the hostile Warden on the striker,
        /// then runs `graph`.
        fn stun(&mut self, ticks: u16, graph: &'static ActionGraph) {
            self.restore(|players| {
                players[0].combat.abilities.auras.push(Aura {
                    ability: ids::SHIELD_BASH,
                    caster: EntityRef::Player(2),
                    remaining: ticks,
                    amount: 0,
                });
            });
            self.zone.set_action_graph(graph).unwrap();
        }

        fn restore(&mut self, edit: impl FnOnce(&mut [mmorpg_core::CanonicalPlayerSnapshot])) {
            let mut state = self.zone.snapshot().unwrap();
            edit(&mut state.players);
            let content = Arc::clone(self.zone.content());
            self.zone = ZoneSimulation::from_snapshot(state, content).unwrap();
        }
    }

    /// A Warden facing the creature, and a hostile level-2 Warden (Shield Bash learned) 15 m
    /// behind it that can stun it.
    fn duel() -> Striker {
        let content = Arc::new(
            ZoneContent::new(
                arena::ground(),
                ZoneAreas::default(),
                vec![CreatureTemplate {
                    behaviour: CreatureBehaviour::Neutral,
                    ..arena::wolf(50_000, [1, 1])
                }],
                vec![arena::spawn(1, WOLF, [0, -150])],
                Vec::new(),
                GRAVEYARD,
            )
            .unwrap()
            .with_creature_abilities(Default::default())
            .unwrap()
            .with_player_combat(mmorpg_core::PlayerCombatPolicy::FreeForAll),
        );
        let mut zone = ZoneSimulation::with_content(ZoneId::new(9), content).unwrap();
        zone.add_player(1).unwrap();
        zone.add_player(2).unwrap();
        zone.apply_command(2, 1, ZoneCommand::ChooseClass { class: 0, sex: 0 })
            .unwrap();
        let mut striker = Striker {
            zone,
            sequence: 0,
            ticks: 0,
        };
        striker.send(ZoneCommand::ChooseClass { class: 0, sex: 0 });
        striker.send(arena::stand(SOUTH));
        striker.tick();
        striker.restore(|players| {
            for player in players.iter_mut() {
                player.combat.level = 2;
                player.combat.health = mmorpg_core::unit::player_max_health(2);
            }
            let [x, y, z] = players[0].position;
            players[1].position = [x, y, z + 1_500.0];
        });
        striker
    }

    /// The baseline with its stun interruption of a basic wind-up replaced by `consequence`.
    fn stun_graph(consequence: Consequence) -> &'static ActionGraph {
        let interruptions: Vec<InterruptionRule> = action::BASELINE
            .interruptions
            .iter()
            .map(|rule| {
                let basic = rule.reaction == ActionId::Stun
                    && rule.interrupted == Interrupted::Action(ActionId::BasicMelee);
                InterruptionRule {
                    consequence: if basic { consequence } else { rule.consequence },
                    ..*rule
                }
            })
            .collect();
        Box::leak(Box::new(ActionGraph {
            interruptions: Box::leak(interruptions.into_boxed_slice()),
            ..action::BASELINE
        }))
    }

    #[test]
    fn a_standing_strike_runs_through_its_wind_up_recovery_and_chain_window() {
        let mut warden = Striker::new(0, SOUTH);
        assert_eq!(warden.offense().action, None);
        warden.send(ZoneCommand::Swing { facing: SOUTH });
        warden.run(10, began);
        let offense = warden.offense();
        assert_eq!(
            (offense.action, offense.phase, offense.strike),
            (
                Some(ActionId::BasicMelee),
                Some(Phase::Startup),
                Some(StrikeVariant::First)
            )
        );
        assert_eq!(
            (offense.recovery_left, offense.follow_up_left),
            (None, None)
        );
        // The hit on the creature ahead recovers on the swing timer, its follow-up open.
        warden.run(120, contact);
        let (offense, combat) = (warden.offense(), warden.combat());
        assert_eq!(offense.phase, Some(Phase::Recovery));
        assert_eq!(offense.strike, Some(StrikeVariant::First));
        assert_eq!(offense.recovery_left, Some(combat.swing_timer));
        assert_eq!(offense.follow_up_left, Some(FOLLOW_UP_WINDOW_TICKS));
        assert_eq!(combat.melee.last, Some(StrikeVariant::First));
        // After the recovery the chain's window still names the strike it continues.
        while warden.combat().swing_timer > 0 {
            warden.tick();
        }
        let (offense, combat) = (warden.offense(), warden.combat());
        assert_eq!(offense.phase, Some(Phase::FollowUp));
        assert_eq!(offense.strike, Some(StrikeVariant::First));
        assert_eq!(offense.follow_up_left, Some(combat.melee.window));
        assert_eq!(combat.melee.window, CHAIN_WINDOW_TICKS);
        for _ in 0..CHAIN_WINDOW_TICKS {
            warden.tick();
        }
        assert_eq!(warden.offense().action, None);
        assert_eq!(warden.combat().melee, mmorpg_core::MeleeState::default());
    }

    #[test]
    fn a_jumping_strike_keeps_its_identity_into_its_landing_recovery() {
        let mut warden = Striker::new(0, SOUTH);
        warden.send(ZoneCommand::Jump);
        warden.tick();
        warden.send(ZoneCommand::Swing { facing: SOUTH });
        warden.run(10, began);
        // In the air the swing timer is still idle: the wind-up alone runs.
        let offense = warden.offense();
        assert_eq!(offense.phase, Some(Phase::Startup));
        assert_eq!(offense.strike, Some(StrikeVariant::Jumping));
        assert_eq!(warden.combat().swing_timer, 0);
        warden.run(120, contact);
        let offense = warden.offense();
        assert_eq!(offense.phase, Some(Phase::Recovery));
        assert_eq!(offense.strike, Some(StrikeVariant::Jumping));
        assert_eq!(offense.recovery_left, Some(SWORD.jumping.interval_ticks));
        // A jumping strike ends the chain, so its identity ends with its recovery.
        for _ in 0..SWORD.jumping.interval_ticks {
            warden.tick();
        }
        assert_eq!(warden.offense().action, None);
        assert_eq!(warden.combat().melee.last, None);
    }

    #[test]
    fn the_graph_decides_what_a_stun_does_to_a_wind_up() {
        let contact_ticks = SWORD.chain[0].contact_ticks;
        let mut outcomes = Vec::new();
        for consequence in [
            Consequence::Cancel,
            Consequence::Preserve,
            Consequence::Suspend,
        ] {
            let mut warden = duel();
            warden.send(ZoneCommand::Swing { facing: SOUTH });
            let (strikes, _) = warden.run(10, began);
            warden.stun(10, stun_graph(consequence));
            let inspection = warden.zone.action_inspection(1).unwrap();
            assert!(inspection.interruptions.iter().any(|report| {
                report.interrupted == Interrupted::Action(ActionId::BasicMelee)
                    && report.consequence == consequence
            }));
            let mut landed = None;
            for _ in 0..u32::from(contact_ticks) + 40 {
                if contact(&warden.tick()) {
                    landed = Some(warden.ticks - strikes[0].tick);
                    break;
                }
            }
            outcomes.push(landed);
        }
        // The baseline cancels it; preserved it lands on time; suspended, after the stun.
        let on_time = u32::from(contact_ticks);
        assert_eq!(outcomes[0], None);
        assert_eq!(outcomes[1], Some(on_time));
        assert!(
            outcomes[2].is_some_and(|late| late > on_time),
            "{outcomes:?}"
        );
        assert_eq!(
            action::BASELINE
                .interruptions
                .iter()
                .find(
                    |rule| rule.interrupted == Interrupted::Action(ActionId::BasicMelee)
                        && rule.reaction == ActionId::Stun
                )
                .map(|rule| (rule.phase, rule.consequence)),
            Some((Some(Phase::Startup), Consequence::Cancel))
        );
    }

    #[test]
    fn a_zone_restored_mid_recovery_reports_the_same_strike() {
        let mut original = Striker::new(0, SOUTH);
        original.send(ZoneCommand::Swing { facing: SOUTH });
        original.run(120, contact);
        original.tick();
        let snapshot = original.zone.snapshot().unwrap();
        assert_eq!(
            snapshot.players[0].combat.melee.last,
            Some(StrikeVariant::First)
        );
        let content = Arc::clone(original.zone.content());
        let restored = ZoneSimulation::from_snapshot(snapshot, content).unwrap();
        assert_eq!(
            restored.action_inspection(1).unwrap().regions,
            original.zone.action_inspection(1).unwrap().regions
        );
    }

    /// How far a running strike's lunge carries a Warden running north under `graph`.
    fn lunge_distance(graph: &'static ActionGraph) -> f32 {
        let mut warden = Striker::new(0, NORTH);
        warden.zone.set_action_graph(graph).unwrap();
        warden.send(arena::walk(NORTH));
        for _ in 0..20 {
            warden.tick();
        }
        warden.send(ZoneCommand::Swing { facing: NORTH });
        let (strikes, _) = warden.run(10, began);
        assert_eq!(strikes[0].variant, StrikeVariant::Running);
        warden.send(arena::stand(NORTH));
        let from = warden.position();
        warden.run(120, contact);
        let to = warden.position();
        (to[0] - from[0]).hypot(to[2] - from[2])
    }

    #[test]
    fn authored_wind_up_movement_shapes_the_lunge_it_drives() {
        // The drive comes first, then the phase's authored movement (#1226): a lunge no
        // longer replaces what the graph authors for the strike's wind-up.
        static HALF_STEERING: [MovementRule; 1] = [MovementRule {
            action: ActionId::BasicMelee,
            phase: Phase::Startup,
            conditions: &[],
            movement: Movement {
                steering_percent: 50,
                momentum_percent: 0,
                max_acceleration_cms2: None,
            },
        }];
        static GRAPH: ActionGraph = ActionGraph {
            movement: &HALF_STEERING,
            ..action::BASELINE
        };
        let free = lunge_distance(&action::BASELINE);
        let authored = lunge_distance(&GRAPH);
        let lunge = SWORD.lunge_units as f32;
        assert!((free - lunge).abs() <= 3., "free lunge {free} of {lunge}");
        assert!(
            (authored - lunge / 2.).abs() <= 6.,
            "half-steered lunge {authored} of {lunge}"
        );
    }
}
