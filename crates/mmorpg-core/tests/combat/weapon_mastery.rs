//! Weapon-family mastery (#915): a Warden's sword practice in real fights becomes durable
//! mastery that unlocks the Leaping Slash, an airborne attack landing into an immediate Heroic
//! Strike. Swinging at air, hitting a foe that never dies and killing trivial foes earn
//! nothing; mastery survives a character reload and the whole journey replays exactly.
use crate::support;

use std::sync::Arc;

use mmorpg_core::ability::ids;
use mmorpg_core::action::{ActionId, ActionInput, Candidate, EdgeId, Outcome, RuleId};
use mmorpg_core::{
    CanonicalZoneSnapshot, CharacterId, CreatureBehaviour, CreatureId, CreatureTemplate,
    CreatureTemplateId, EntityRef, Mastery, StrikeAim, StrikeHeight, StrikeSide, ZoneAreas,
    ZoneCommand, ZoneContent, ZoneEvent, ZoneId, ZoneSimulation,
};
use support::arena::{self, GRAVEYARD, WOLF};

const EAST: u16 = 16_384;
const SOUTH: u16 = 32_768;
/// A foe with far more health than any test spends on it.
const POST: CreatureTemplateId = CreatureTemplateId::new(2);
const FOE: EntityRef = EntityRef::Creature(CreatureId::new(1));
const TOUGH: EntityRef = EntityRef::Creature(CreatureId::new(2));

/// A Greyhaven Timber Wolf's first level (42 health) that respawns as its corpse fades, 1.5 m
/// south, and a level-1 post 1.5 m west that never falls. Both fight back.
fn content() -> Arc<ZoneContent> {
    let wolf = CreatureTemplate {
        behaviour: CreatureBehaviour::Neutral,
        respawn_ticks: mmorpg_core::unit::CORPSE_TICKS,
        ..arena::wolf(42, [1, 2])
    };
    let post = CreatureTemplate {
        id: POST,
        name: "Test Post".into(),
        swing_ticks: mmorpg_core::ticks_from_ms(10_000) as _,
        ..wolf.clone()
    };
    let post = CreatureTemplate {
        health: 100_000,
        ..post
    };
    Arc::new(
        ZoneContent::new(
            arena::ground(),
            ZoneAreas::default(),
            vec![wolf, post],
            vec![
                arena::spawn(1, WOLF, [0, -150]),
                arena::spawn(2, POST, [-150, 0]),
            ],
            Vec::new(),
            GRAVEYARD,
        )
        .unwrap()
        .with_creature_abilities(Default::default())
        .unwrap(),
    )
}

struct Warden {
    zone: ZoneSimulation,
    content: Arc<ZoneContent>,
    sequence: u32,
}

impl Warden {
    /// A settled Warden of `level` at the origin, untrained.
    fn new(level: u8) -> Self {
        let content = content();
        let mut zone = ZoneSimulation::with_content(ZoneId::new(9), Arc::clone(&content)).unwrap();
        zone.add_player(1).unwrap();
        let mut warden = Self {
            zone,
            content,
            sequence: 0,
        };
        warden.send(ZoneCommand::ChooseClass { class: 0, sex: 0 });
        warden.send(arena::stand(SOUTH));
        warden.ticks(20);
        let mut state = warden.zone.snapshot().unwrap();
        let player = &mut state.players[0].combat;
        player.level = level;
        player.health = mmorpg_core::unit::player_max_health(level);
        warden.zone = ZoneSimulation::from_snapshot(state, Arc::clone(&warden.content)).unwrap();
        warden
    }

    fn send(&mut self, command: ZoneCommand) {
        self.sequence += 1;
        self.zone.apply_command(1, self.sequence, command).unwrap();
    }

    fn tick(&mut self) -> Vec<ZoneEvent> {
        self.zone.advance_tick().unwrap();
        self.zone.snapshot_for_player(1).unwrap().events
    }

    fn ticks(&mut self, count: u32) {
        for _ in 0..count {
            self.tick();
        }
    }

    fn mastery(&self) -> Mastery {
        self.zone.snapshot_for_player(1).unwrap().viewer.mastery
    }

    fn banked(&self) -> usize {
        self.zone.snapshot().unwrap().players[0].practice.len()
    }

    /// The life of creature `id`: its generation, and whether it is alive.
    fn life(&self, id: u32) -> (u64, bool) {
        let state = self.zone.snapshot().unwrap();
        let creature = state
            .creatures
            .iter()
            .find(|creature| creature.creature_id == CreatureId::new(id))
            .unwrap();
        (
            creature.generation,
            creature.life == mmorpg_core::CreatureLife::Alive,
        )
    }

    /// Fights `foe` for `ticks`: basic strikes toward it twice a second and a Heroic Strike
    /// every 3 s, as a player presses them.
    fn fight(&mut self, foe: EntityRef, ticks: u32) {
        for tick in 0..ticks {
            if tick % 30 == 0
                && let Some(swing) = arena::swing_at(&self.zone, 1, foe)
            {
                self.send(swing);
            }
            if tick % 180 == 90
                && let Some((facing, _)) = arena::aim_at(&self.zone, 1, foe)
            {
                self.send(ZoneCommand::DirectedStrike {
                    ability: ids::HEROIC_STRIKE.get(),
                    facing,
                    aim: StrikeAim::new(StrikeSide::Centre, StrikeHeight::Middle),
                });
            }
            self.tick();
        }
    }

    /// Kills wolves one life after another until the sword reaches mastery 1, at most `limit`;
    /// the wolves killed.
    fn hunt(&mut self, limit: u64) -> u64 {
        let (first, _) = self.life(1);
        while self.mastery().level == 0 {
            let (life, alive) = self.life(1);
            assert!(life - first < limit, "mastery within {limit} wolves");
            if alive {
                self.fight(FOE, 30);
            } else {
                self.ticks(30);
            }
        }
        let (life, alive) = self.life(1);
        life - first + u64::from(!alive)
    }

    /// Jumps, strikes in the air and lands: the outcome of a Heroic Strike pressed each tick of
    /// the landing, first to last.
    fn leap(&mut self) -> Vec<Outcome> {
        self.send(arena::stand(EAST));
        self.ticks(120);
        self.send(ZoneCommand::Jump);
        self.ticks(6);
        self.send(ZoneCommand::Swing { facing: EAST });
        let movement = |zone: &ZoneSimulation| {
            zone.action_inspection(1).unwrap().regions
                [mmorpg_core::action::Region::Movement as usize]
                .action
        };
        while movement(&self.zone) != Some(ActionId::Landing) {
            self.tick();
        }
        let mut outcomes = Vec::new();
        while movement(&self.zone) == Some(ActionId::Landing) {
            outcomes.push(
                self.zone
                    .arbitrate_input(1, ActionInput::WardenMelee)
                    .unwrap()
                    .outcome,
            );
            self.tick();
        }
        outcomes
    }
}

/// The earning journey up to the unlock, optionally restored from its canonical state midway:
/// the wolves it took and the final state.
fn journey(restore: bool) -> (u64, CanonicalZoneSnapshot) {
    let mut warden = Warden::new(1);
    warden.fight(FOE, 300);
    if restore {
        warden.zone =
            ZoneSimulation::from_snapshot(warden.zone.snapshot().unwrap(), warden.content.clone())
                .unwrap();
    }
    let wolves = warden.hunt(12);
    (wolves, warden.zone.snapshot().unwrap())
}

#[test]
fn real_fights_unlock_the_leaping_slash_and_reload_keeps_it() {
    let mut warden = Warden::new(1);
    // Blows bank practice against the living wolf, but only its death pays it.
    warden.fight(FOE, 120);
    assert_eq!(warden.life(1).0, 1, "the first wolf still stands");
    assert_eq!(warden.banked(), 1);
    assert_eq!(warden.mastery(), Mastery::default());
    let wolves = warden.hunt(12);
    eprintln!(
        "sword mastery 1 after {wolves} wolves, tick {}",
        warden.zone.snapshot().unwrap().tick
    );
    assert!((3..=8).contains(&wolves), "{wolves} wolves");
    assert_eq!(warden.mastery().level, 1);

    // The mastered Warden's airborne strike lands into an immediate Heroic Strike...
    let outcomes = warden.leap();
    let leaping = Outcome::Proceed(Candidate::Edge(EdgeId::LeapingSlash));
    let opens = outcomes
        .iter()
        .position(|outcome| *outcome == leaping)
        .expect("the Leaping Slash opens on the landing");
    assert_eq!(
        outcomes[0],
        Outcome::WaitForRule(RuleId::LandingHoldsMelee),
        "the touchdown itself still holds melee"
    );
    // ...where an untrained Warden waits out the whole landing.
    let mut novice = Warden::new(1);
    let held = novice.leap();
    assert_eq!(held.len(), outcomes.len());
    assert!(
        held.iter()
            .all(|outcome| *outcome == Outcome::WaitForRule(RuleId::LandingHoldsMelee))
    );

    // The Heroic Strike pressed in the window resolves at once and ends the landing.
    let mut reloaded = {
        let record = warden
            .zone
            .capture_character(1, CharacterId::new("sword-master").unwrap())
            .unwrap();
        assert_eq!(record.mastery.sword, warden.mastery());
        let mut zone =
            ZoneSimulation::with_content(ZoneId::new(9), warden.content.clone()).unwrap();
        zone.admit_character(1, &record).unwrap();
        Warden {
            zone,
            content: warden.content.clone(),
            sequence: 0,
        }
    };
    assert_eq!(reloaded.mastery(), warden.mastery(), "reload keeps mastery");
    let again = reloaded.leap();
    assert_eq!(again.iter().position(|o| *o == leaping), Some(opens));
    reloaded.send(ZoneCommand::Jump);
    reloaded.ticks(6);
    reloaded.send(ZoneCommand::Swing { facing: EAST });
    while reloaded.zone.action_inspection(1).unwrap().regions
        [mmorpg_core::action::Region::Movement as usize]
        .action
        != Some(ActionId::Landing)
    {
        reloaded.tick();
    }
    reloaded.ticks(u32::try_from(opens).unwrap());
    reloaded.send(ZoneCommand::DirectedStrike {
        ability: ids::HEROIC_STRIKE.get(),
        facing: EAST,
        aim: StrikeAim::new(StrikeSide::Centre, StrikeHeight::Middle),
    });
    let events = reloaded.tick();
    assert!(
        events
            .iter()
            .any(|event| matches!(event, ZoneEvent::StrikeSwept { .. })),
        "the Leaping Slash swings: {events:?}"
    );
}

#[test]
fn swinging_at_air_an_unkilled_foe_or_trivial_foes_earns_nothing() {
    // Two minutes of swings at nothing, then at a post that never falls.
    let mut warden = Warden::new(1);
    warden.send(arena::stand(EAST));
    for _ in 0..240 {
        warden.send(ZoneCommand::Swing { facing: EAST });
        warden.ticks(30);
    }
    assert_eq!(warden.banked(), 0, "whiffs bank nothing");
    warden.fight(TOUGH, 3_600);
    assert_eq!(warden.banked(), 1, "the post's blows stay banked");
    assert_eq!(warden.mastery(), Mastery::default());

    // A level-6 Warden kills the level-1 wolves: trivial, they teach nothing.
    let mut veteran = Warden::new(6);
    for _ in 0..4 {
        let (life, _) = veteran.life(1);
        while veteran.life(1).0 == life {
            if veteran.life(1).1 {
                veteran.fight(FOE, 30);
            } else {
                veteran.ticks(30);
            }
        }
    }
    assert_eq!(veteran.banked(), 0);
    assert_eq!(veteran.mastery(), Mastery::default());
}

#[test]
fn the_journey_replays_exactly_through_a_restore() {
    let straight = journey(false);
    assert_eq!(journey(false), straight);
    assert_eq!(journey(true), straight);
}
