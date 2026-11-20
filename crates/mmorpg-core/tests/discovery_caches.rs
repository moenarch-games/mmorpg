//! Greyhaven's optional discoveries (#913): each site has a clue placed on the way in, guards
//! that keep its cache shut while they live, and an authored reward a character takes once,
//! through replay, canonical recovery and a saved and reloaded character.
use std::collections::BTreeSet;
use std::sync::Arc;

use mmorpg_core::greyhaven_vale::discoveries::{self, FEN_CACHE, HUNTERS_CACHE, LOOKOUT_CACHE};
use mmorpg_core::greyhaven_vale::units::templates::OLD_SCARBACK;
use mmorpg_core::{
    CharacterId, ClassChoice, CreatureAi, CreatureBehaviour, CreatureLife, DiscoveryCache,
    PLAYER_HALF_EXTENTS_UNITS, PlayerClass, RewardKey, Sex, ZoneId, ZoneSimulation, greyhaven_vale,
};

const WARDEN: ClassChoice = ClassChoice {
    class: PlayerClass::Warden,
    sex: Sex::Male,
};

/// Each cache's clue and the art its reward lies in.
const SITES: [(u32, &str, &str); 3] = [
    (
        HUNTERS_CACHE,
        "discoveries.kit.hunter-clue",
        "discoveries.kit.hunter-chest",
    ),
    (
        LOOKOUT_CACHE,
        "discoveries.kit.lookout-banner",
        "discoveries.kit.lookout-platform",
    ),
    (
        FEN_CACHE,
        "discoveries.kit.fen-marker",
        "discoveries.kit.supply-crates",
    ),
];

fn cache(id: u32) -> DiscoveryCache {
    discoveries::caches()
        .into_iter()
        .find(|cache| cache.id == id)
        .expect("an authored cache")
}

/// `zone` restored with player 1's feet at `feet`.
fn moved(zone: &ZoneSimulation, feet: [i32; 3]) -> ZoneSimulation {
    let mut state = zone.snapshot().unwrap();
    state.players[0].position = mmorpg_core::physics::from_units([
        feet[0],
        feet[1] + PLAYER_HALF_EXTENTS_UNITS[1],
        feet[2],
    ]);
    ZoneSimulation::from_snapshot(state, Arc::clone(zone.content())).unwrap()
}

/// `zone` restored with `cache`'s guards dead and gone.
fn felled(zone: &ZoneSimulation, cache: &DiscoveryCache) -> ZoneSimulation {
    let mut state = zone.snapshot().unwrap();
    let tick = state.tick;
    for creature in &mut state.creatures {
        if cache.guards.contains(&creature.creature_id) {
            creature.life = CreatureLife::Despawned { died_at: tick };
            creature.health = 0;
            creature.position = [0.0; 3];
            creature.velocity = [0.0; 3];
            creature.ai = CreatureAi::Idle {
                timer: 0,
                destination: None,
            };
            creature.threat.clear();
            creature.attack = None;
        }
    }
    ZoneSimulation::from_snapshot(state, Arc::clone(zone.content())).unwrap()
}

/// `(copper, count of the cache's item, opened caches)` of player 1.
fn rewards(zone: &ZoneSimulation, cache: &DiscoveryCache) -> (u32, u32, BTreeSet<u32>) {
    let player = zone.snapshot().unwrap().players.remove(0);
    (
        player.copper,
        player.inventory.count(cache.item),
        player.caches,
    )
}

fn ticks(zone: &mut ZoneSimulation, count: u32) {
    for _ in 0..count {
        zone.advance_tick().unwrap();
    }
}

/// Each site's clue and reward art stand by its cache, so the clue is read on the way in.
#[test]
fn every_cache_has_its_clue_and_its_art_on_the_way_in() {
    #[derive(serde::Deserialize)]
    struct Placement {
        asset: String,
        position: [i32; 3],
    }
    #[derive(serde::Deserialize)]
    struct Dressing {
        placements: Vec<Placement>,
    }
    let dressing: Dressing = serde_json::from_str(include_str!(
        "../../../content/greyhaven/dressing/discoveries.json"
    ))
    .unwrap();
    let distance = |a: [i32; 3], b: [i32; 3]| {
        let (dx, dz) = (f64::from(a[0] - b[0]), f64::from(a[2] - b[2]));
        dx.hypot(dz)
    };
    for (id, clue, art) in SITES {
        let cache = cache(id);
        let near = |asset: &str, metres: f64| {
            dressing
                .placements
                .iter()
                .filter(|p| p.asset == asset)
                .map(|p| distance(p.position, cache.at))
                .any(|units| units <= metres * 100.0)
        };
        assert!(near(clue, 20.0), "{}: no clue within 20 m", cache.name);
        assert!(near(art, 3.0), "{}: no reward art within 3 m", cache.name);
    }
}

/// Each cache is guarded by aggressive creatures close by; the hunter's cache by an elite
/// above every level the Wolfrun Woods' wolves roll, a risk to take before being ready.
#[test]
fn every_cache_is_guarded_and_one_by_an_elite() {
    let content = greyhaven_vale::content();
    for cache in discoveries::caches() {
        assert!(!cache.guards.is_empty(), "{}", cache.name);
        for guard in &cache.guards {
            assert!(discoveries::SPAWN_IDS.contains(&guard.get()));
            let spawn = content.creature_spawn(*guard).unwrap();
            let template = content.creature_template(spawn.template).unwrap();
            assert_eq!(template.behaviour, CreatureBehaviour::Aggressive);
            let (dx, dz) = (
                i64::from(spawn.position[0] - cache.at[0]),
                i64::from(spawn.position[1] - cache.at[2]),
            );
            assert!(dx * dx + dz * dz < 1_000_i64.pow(2), "{}", cache.name);
        }
    }
    let scarback = content.creature_template(OLD_SCARBACK).unwrap();
    assert!(scarback.elite && scarback.min_level == 5);
}

/// Every cache stays shut while a guard lives, then pays its reward once per character:
/// staying, replay, recovery and a reloaded character pay nothing more.
#[test]
fn every_cache_pays_once_per_character_through_replay_recovery_and_a_saved_character() {
    for id in [HUNTERS_CACHE, LOOKOUT_CACHE, FEN_CACHE] {
        let cache = cache(id);
        let mut zone =
            ZoneSimulation::with_content(ZoneId::new(1), greyhaven_vale::content()).unwrap();
        zone.enable_reward_tracking(64, 16).unwrap();
        zone.create_character(1, WARDEN).unwrap();
        ticks(&mut zone, 2);
        let (copper, items, opened) = rewards(&zone, &cache);
        assert!(opened.is_empty());

        // At the cache with its guards alive: shut.
        let mut guarded = moved(&zone, cache.at);
        ticks(&mut guarded, 1);
        assert_eq!(rewards(&guarded, &cache), (copper, items, BTreeSet::new()));

        // With the guards down it opens on the next tick, once.
        let before = moved(&felled(&zone, &cache), cache.at);
        let mut zone =
            ZoneSimulation::from_snapshot(before.snapshot().unwrap(), Arc::clone(before.content()))
                .unwrap();
        ticks(&mut zone, 1);
        let paid = (copper + cache.copper, items + 1, BTreeSet::from([id]));
        assert_eq!(rewards(&zone, &cache), paid, "{}", cache.name);
        assert!(
            !zone
                .reward_history()
                .unwrap()
                .allows(1, RewardKey::Cache { cache: id })
        );
        ticks(&mut zone, 30);
        assert_eq!(rewards(&zone, &cache), paid, "staying pays nothing more");

        // Replay from before the opening reaches exactly the same state.
        let mut replay =
            ZoneSimulation::from_snapshot(before.snapshot().unwrap(), Arc::clone(before.content()))
                .unwrap();
        ticks(&mut replay, 31);
        assert_eq!(replay.snapshot().unwrap(), zone.snapshot().unwrap());

        // Canonical recovery keeps it opened.
        let mut recovered =
            ZoneSimulation::from_snapshot(zone.snapshot().unwrap(), Arc::clone(zone.content()))
                .unwrap();
        ticks(&mut recovered, 5);
        assert_eq!(rewards(&recovered, &cache), paid);

        // A saved and reloaded character standing at the open cache takes nothing again.
        let record = recovered
            .capture_character(1, CharacterId::new("explorer").unwrap())
            .unwrap();
        assert_eq!(record.caches, BTreeSet::from([id]));
        let mut reloaded =
            ZoneSimulation::with_content(ZoneId::new(1), greyhaven_vale::content()).unwrap();
        reloaded.admit_character(1, &record).unwrap();
        let mut reloaded = moved(&felled(&reloaded, &cache), cache.at);
        ticks(&mut reloaded, 5);
        assert_eq!(rewards(&reloaded, &cache), paid, "{}", cache.name);
    }
}
