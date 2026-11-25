#![forbid(unsafe_code)]
// No platform-libm float functions in authoritative code (clippy.toml, #1109).
#![cfg_attr(not(test), warn(clippy::disallowed_methods))]

//! Deterministic zone-local gameplay: movement, units, combat, creature AI
//! and the interest/visibility policy of player projections.
//!
//! [`ZoneSimulation`] advances in the fixed tick order documented on
//! [`ZoneSimulation::advance_tick`]. Physics integration and collision stay in
//! Physics Engine, embedded in [`physics`] (#648; Rapier only under the `physics-ab`
//! diagnostics); wire formats live in `mmorpg-protocol`.

pub mod ability;
pub mod action;
mod ai;
mod areas;
mod aura;
mod boss;
mod boulder;
mod breakable;
pub mod buoyancy;
mod cache;
mod carry;
pub mod cart;
mod casting;
mod character;
mod chat;
mod combat;
mod combo;
mod construction;
mod content;
mod corpse;
mod corpse_loot;
mod corridor;
mod creature;
mod creature_attack;
mod creature_store;
pub mod crossing;
mod den;
mod depths;
mod depths_boss;
mod door;
mod ephemeral;
mod equipment;
mod events;
mod evidence;
mod finds;
mod flank;
pub mod flight;
mod flock;
mod forced;
mod gathering;
#[doc(hidden)]
pub mod golden;
mod guard;
mod home;
pub mod hunt;
mod interest;
mod inventory;
mod landmark;
mod loot;
pub mod lure;
mod mastery;
mod mechanism;
mod melee;
mod minion;
mod mobility;
mod movable;
mod nav;
mod npc_quest;
mod party;
mod patrol;
#[cfg(feature = "physics-ab")]
pub mod physics_ab;
mod physique;
mod platform;
mod prediction;
mod progression;
mod projectile;
mod projection;
mod pvp;
mod quest;
pub mod raid;
mod reckoning;
mod revisioned;
mod rewards;
mod rewind;
mod routine;
mod seat;
mod shapes;
mod signal;
mod snapshot;
mod stance;
mod strike;
mod structure;
mod summoned;
pub mod supply;
mod swing;
mod threat_cache;
mod transfer;
pub mod trap;
mod vendor;
pub mod water;
mod waystone;
mod zone;
mod zone_views;

pub use ability::{
    ABILITY_CATALOG, Ability, AbilityEffect, AbilityId, AbilityRange, AbilityUser, AuraKind,
    AuraSpec, CORPSE_WINDOW_TICKS, CastTime, GLOBAL_COOLDOWN_TICKS, MAX_AURAS, MAX_COOLDOWNS, Shot,
    ability_by_id,
};
pub use areas::{Area, AreaId, MAX_AREA_NAME_BYTES, ZoneAreas};
pub use aura::{Aura, CastState, Cooldown};
pub use boss::{BossEncounter, BossRules, MAX_ENCOUNTER_PLAYERS, MAX_SCALING_PARTICIPANTS};
pub use boulder::{CanonicalBoulder, IMPACT_SPEED, RETURN_TICKS, WALK_PUSH_SPEED};
pub use breakable::{
    DEBRIS_TICKS, Debris, DebrisPiece, DebrisView, EXPLOSION_DAMAGE, EXPLOSION_KNOCKBACK_UNITS,
    EXPLOSION_RADIUS_UNITS, MAX_DEBRIS, MAX_DEBRIS_PER_TICK, MAX_VISIBLE_DEBRIS, debris_pieces,
};
pub use cache::{CACHE_HEIGHT_UNITS, DiscoveryCache};
pub use carry::{
    LOAD_HEIGHT_UNITS, LOAD_REACH_UNITS, LoadKind, LoadSpot, LoadSpotId, LoadState, LoadView,
    MAX_LOAD_REACH_UNITS, MAX_VISIBLE_LOADS, MAX_ZONE_LOADS, PLACED_LOAD_TICKS, PlacedLoad,
};
pub use cart::{CartOutcome, CartRun};
pub use character::{
    CHARACTER_RECORD_SCHEMA_VERSION, CharacterId, CharacterLocation, CharacterRecord, DepthsRecord,
    MAX_CHARACTER_ID_BYTES,
};
pub use chat::{
    CHAT_INTERVAL_TICKS, ChatChannel, ChatLine, ChatMessage, ChatText, Emote, MAX_CHAT_BYTES,
    MAX_CHAT_PER_TICK, SAY_RANGE_UNITS, YELL_RANGE_UNITS,
};
pub use climb::{ClimbSurfaces, Grip, Ledge, Traversal, TraversalView};
pub use combo::{COMBO_IMPACT_TICKS, COMBO_WINDOW_TICKS, ComboOpening, ComboPhase};
pub use construction::{
    ConstructionSite, ConstructionState, MAX_CONSTRUCTION_MATERIALS, MAX_CONSTRUCTION_SITES,
    STAND_TICKS as CONSTRUCTION_STAND_TICKS,
};
pub use content::greyhaven_vale::{self, greyhaven_vale_definition};
pub use content::redbrand_depths;
pub use content::seam_test::{self, content_for_zone};
pub use content::{
    CharacterPersistence, CreatureBehaviour, CreatureFamily, CreatureSpawn, CreatureTemplate,
    LARGE_CREATURE_HALF_WIDTH_UNITS, MAX_CONTENT_COORDINATE_UNITS, MAX_CREATURE_HALF_EXTENT_UNITS,
    MAX_CREATURE_SPAWNS, MAX_CREATURE_TEMPLATES, MAX_NPCS, MAX_STATIC_COLLIDERS,
    MAX_TERRAIN_HEIGHT_UNITS, MAX_UNIT_NAME_BYTES, MAX_WANDER_RADIUS_UNITS, Npc, NpcRole,
    PlayerCombatPolicy, PropKind, SpawnGrid, StaticCollider, StaticMesh, Terrain, UNITS_PER_METRE,
    XzBounds, ZoneContent, ZoneDefinition,
};
pub use corpse::CORPSE_SPEND_REACH_UNITS;
pub use corpse_loot::{LOOT_REACH_UNITS, LootClaim, LootView};
pub use corridor::{CORRIDOR_TURN_BACK_UNITS, Departure, HandoffCorridor};
pub use creature::{
    AGGRO_MAX_RADIUS_UNITS, AGGRO_MIN_RADIUS_UNITS, AGGRO_PER_LEVEL_UNITS, AGGRO_RADIUS_UNITS,
    AMBUSH_RADIUS_UNITS, ARRIVAL_RADIUS_UNITS, ASSIST_RADIUS_UNITS,
    CREATURE_EVADE_SPEED_SUBUNITS_PER_TICK, CREATURE_RUN_SPEED_SUBUNITS_PER_TICK,
    CREATURE_WALK_SPEED_SUBUNITS_PER_TICK, EVADE_TIMEOUT_TICKS, LEASH_RADIUS_UNITS,
    MAX_THREAT_ENTRIES, TERRITORY_RADIUS_UNITS, WANDER_WAIT_TICKS, WANDER_WALK_TIMEOUT_TICKS,
};
pub use creature_attack::{
    ADDER_RECOVER_TICKS, ADDER_SLITHER_SPEED_SUBUNITS_PER_TICK, ADDER_WARN_TICKS, BAT_DIVE_TICKS,
    BAT_DIVE_UNITS, BAT_FLIGHT_SPEED_SUBUNITS_PER_TICK, BAT_RECOVER_TICKS, CRAYFISH_CONTACT_TICKS,
    CRAYFISH_RECOVER_TICKS, CRAYFISH_SCUTTLE_SPEED_SUBUNITS_PER_TICK, CREATURE_ATTACK_SLACK_UNITS,
    CreatureAttack, HEAVY_ATTACK_ODDS, Lunge, POUNCE_LEAP_SPEED_SUBUNITS_PER_TICK,
    POUNCE_LEAP_TICKS, POUNCE_LEAP_UNITS, POUNCE_RECOVER_TICKS, PendingAttack, STAG_ALERT_TICKS,
    STAG_CHARGE_SPEED_SUBUNITS_PER_TICK, STAG_CHARGE_TICKS, STAG_CHARGE_UNITS, STAG_RECOVER_TICKS,
    contact_ticks, lunge,
};
pub use crossing::{
    CrossingRaider, CrossingRun, MAX_CROSSING_RAIDERS, MAX_CROSSING_ROUTE_POINTS, MAX_CROSSINGS,
    WagonCrossing,
};
pub use den::{DEN_LEG_TICKS, FLEE_RADIUS_UNITS, HUNT_RADIUS_UNITS, WildlifeDen};
pub use depths::{
    DepthsChamber, DepthsRun, DepthsRunStart, DepthsSite, DepthsSpawn, DepthsThreshold,
    ThresholdRequest, WIPE_DELAY_TICKS, depths_arrival,
};
pub use depths_boss::{
    BossAttempt, BossDrop, BossOutcome, BossProgress, BossReport, DepthsBoss, DepthsDropView,
    DepthsLootView, DropState, DropView, MAX_BOSS_DROPS, RollChoice, RollWin, equippable,
    pick_winner, resolve_roll, roll_for,
};
pub use door::{
    CanonicalDoor, DOOR_COOLDOWN_TICKS, DOOR_REACH_UNITS, Door, DoorBox, DoorId, DoorView,
    NPC_SIGHT_HEIGHT_UNITS,
};
pub use entity::{CreatureId, CreatureTemplateId, EntityKind, EntityRef, MinionId, NpcId};
pub use ephemeral::EphemeralCharacterBuild;
pub use equipment::{
    EQUIPMENT_SLOTS, Equipment, EquipmentSlot, HEALTH_PER_STAMINA, ItemStats, MAX_EQUIPPED_STAMINA,
    PRIMARY_STAT_PER_DAMAGE, StatTotals,
};
pub use events::{ErrorCode, MAX_EVENTS_PER_PLAYER, ZoneEvent};
pub use evidence::{
    EVIDENCE_HEIGHT_UNITS, EvidenceId, EvidenceSpot, MAX_EVIDENCE_DIRECTIONS_BYTES,
    MAX_EVIDENCE_REACH_UNITS,
};
pub use finds::{
    ITEM_FINDS, ItemFind, ItemQuality, ItemTrait, NamedWeapon, STANDARD_FIND_WEIGHT, find_text,
    item_find, roll_variant, rolls_as, shaped_strike,
};
pub use flank::{
    CREATURE_TURN_PER_TICK, FLANK_DAMAGE_PERCENT, FLANK_FRONT_HALF_ARC, FLANK_WARY_TICKS,
    WARY_TURN_PER_TICK, flanks,
};
pub use flock::{
    BITE_REACH_UNITS, GAP_HOLD_TICKS, GAP_REACH_UNITS, HedgeWatch, MAX_FLOCK, MAX_PROWLERS,
    MAX_STRAYS, PEN_REST_TICKS, Prowler, RECOVER_TICKS, SHY_RADIUS_UNITS, STRAY_TIMEOUT_TICKS,
    STRAY_UNITS_PER_TICK, Stray, StrayState, WATCH_TICKS, WatchState,
};
pub use forced::{
    CHARGE_MAX_TICKS, CHARGE_SPEED_SUBUNITS_PER_TICK, FORCED_IMMUNITY_TICKS, ForcedMove,
    ForcedState, KNOCKBACK_TICKS, SHOVE_UNITS,
};
pub use gathering::{
    BOAR_HIDE, BOAR_TUSK, GATHER_REACH_UNITS, GatherNode, GatheringState, HOLLOWPINE_BOAR_SPEAR,
    IRON_ORE, skin_yield,
};
pub use guard::{
    GUARD_ARC_HALF_WIDTH, GUARD_ARC_OFFSET, GUARD_BREAK_IMPACT, GUARD_BREAK_TICKS,
    GUARD_SPEED_PERCENT, GuardState, STRIKE_RECOVERY_TICKS, lane_of,
};
pub use home::{
    MAX_WAKE_WAYPOINTS, NpcWake, PRIVATE_DOOR_CLOSE_TICKS, PrivateHome, WAKE_IDLE_TICKS,
    WAKE_RISE_TICKS,
};
pub use hunt::{HuntOutcome, HuntOuting};
pub use interest::{InterestMaintenanceStats, InterestQueryStats, PlayerProjection};
pub use inventory::{
    GreatWeapon, INVENTORY_SLOTS, ITEM_CATALOG, Inventory, InventoryError, ItemId, ItemStack,
    ItemTemplate, item_template,
};
pub use landmark::{Landmark, LandmarkKind};
pub use loot::{
    LootOutcome, LootRewards, LootRolls, LootSettlementError, LootTable, LootTableError,
    MAX_LOOT_OUTCOMES, loot_table, settle_loot,
};
pub use lure::{LURE_RADIUS_UNITS, LURE_TICKS, Lure, LureState, MAX_LURES};
pub use mastery::{
    BLOW_PRACTICE, LEAPING_SLASH_LEVEL, MAX_MASTERY_LEVEL, MAX_PRACTICE, Mastery, PRACTICE_PER_FOE,
    Practice, TECHNIQUE_PRACTICE, TRIVIAL_LEVELS_BELOW, WeaponMastery, practice_to_next_level,
    valid_practice,
};
pub use mechanism::{
    MAX_MECHANISM_REACH_UNITS, MAX_SEAL_MECHANISMS, MAX_SEALS, MAX_SEQUENCE_MECHANISMS, Mechanism,
    MechanismId, MechanismSequence, SEAL_OPEN_TICKS, SEAL_WINDOW_TICKS, Seal, SealState, SealView,
    SequenceId, WARD_EXPOSED_TICKS, Ward, WardState, WardView,
};
pub use melee::{JUMP_STRIKE_AIR_TICKS, MeleeState, PendingStrike};
pub use minion::{
    COMPANION_HALF_EXTENTS_UNITS, CanonicalMinion, DEFEND_RADIUS_UNITS, ENGAGE_RADIUS_UNITS,
    FORMATION_RADIUS_UNITS, MAX_MINIONS, MAX_MINIONS_PER_SUMMONER, MINION_HALF_EXTENTS_UNITS,
    Minion, MinionOrder, ORDER_RANGE_UNITS, RAISE_DEAD_LIMIT, is_companion, minion_ability,
    minion_half_extents,
};
pub use nav::NAV_CELL_UNITS;
pub use npc_quest::{
    Admission, Confrontation, ESCORT_GAP_UNITS, ESCORT_GATHER_UNITS, ESCORT_LEASH_TICKS,
    ESCORT_LEASH_UNITS, ESCORT_REST_TICKS, ESCORT_TRAIL_SPACING_UNITS, ESCORT_UNITS_PER_TICK,
    EscortCompanion, EscortState, MAX_ESCORT_COMPANIONS, MAX_ESCORT_TRAIL, NpcAttitude,
};
pub use party::{
    MAX_PARTY_MEMBERS, MAX_PARTY_NAME_BYTES, PartyInvite, PartyMember, PartyRefusal, PartyRoster,
    PartyState, valid_name as valid_party_name,
};
pub use patrol::{PatrolMember, PatrolRole};
pub use physique::{
    InvalidPhysique, MAX_PHYSIQUE_MODIFIER_BP, PHYSIQUE_ATTRIBUTES, PHYSIQUE_COEFFICIENTS,
    PHYSIQUE_NEUTRAL, PHYSIQUE_PARAMS, PHYSIQUE_SCALE, PHYSIQUE_STAT_PARAMS, Physique,
    PhysiqueAttribute, PhysiqueParam, PhysiquePreset,
};
pub use platform::{Platform, PlatformId, PlatformPhase};
pub use prediction::MovementPredictor;
pub use progression::{
    MAX_PLAYER_LEVEL, discovery_experience, experience_to_next_level, kill_experience,
};
pub use projectile::{
    AIM_ASSIST_COS, ARROW_SPEED_UNITS, BOLT_SPEED_UNITS, BOMB_FUSE_TICKS, BOMB_RESTITUTION,
    BURST_TICKS, Flight, KNIFE_SPEED_UNITS, LAUNCH_HEIGHT_UNITS, MAX_PROJECTILES,
    MAX_VISIBLE_PROJECTILES, MISSILE_FLIGHT_HEIGHT_UNITS, MISSILE_SPEED_UNITS,
    PIERCE_DAMAGE_PERCENT, PIERCE_HITS, Projectile, ProjectileKind, ProjectileView,
    RICOCHET_BOUNCES, STUCK_IN_UNIT_TICKS, STUCK_TICKS, THROW_RANGE_UNITS, THROW_RISE_UNITS,
    THROW_SPEED_UNITS, THROWN_FLIGHT_TICKS,
};
pub use projection::{
    AuraView, CastView, EntityFlags, EntitySnapshot, GuardView, ProjectionWork, ResourceView,
    SheetStamp, SheetStamps, TargetDetail, ViewerState, ZoneSnapshot,
};
pub use quest::{
    DecisionOption, MAX_DECISION_BYTES, MAX_DECISION_COPPER, MAX_DECISION_OPTIONS,
    MAX_QUEST_EXPERIENCE, MAX_QUEST_LOG, MAX_QUEST_NAME_BYTES, MAX_QUEST_OBJECTIVES,
    MAX_QUEST_REQUIRES, MAX_QUEST_SUPPLIES, MAX_QUEST_TEXT_BYTES, MAX_REWARD_CHOICES, NpcDeparture,
    NpcMarker, QUEST_REACH_UNITS, Quest, QuestContentError, QuestEntry, QuestId, QuestLog,
    QuestMarker, QuestObjective, QuestRewards, QuestSheet, QuestSupply,
};
pub use raid::{
    AidChore, AidKind, AidTask, Civilian, RaidApproach, RaidIncident, RaidLeg, RaidMember,
    RaidNotice, RaidOutcome, RaidPrep, RaidResolution, RaidSite, RaidState, RaidView,
};
pub use ramp::{MAX_RAMPS, Ramp, RampRise};
pub use reckoning::{Harm, MAX_RECKONING_POOLS, RECKONING_ALLY_RADIUS};
pub use rewards::{
    DEFAULT_MAX_KNOWN_RECEIPTS, DEFAULT_MAX_TICK_GRANTS, DeferredReward, IncidentFloors,
    MAX_DEFERRED_REWARDS, RECEIPTS_PER_CHARACTER, RewardGrant, RewardHistory, RewardKey,
    RewardPayout, ZONE_PAYOUTS_PER_PLAYER,
};
pub use rewind::{CLIENT_INTERPOLATION_TICKS, HitRewind, MAX_HIT_REWIND_MS, RewindFrame};
pub use rock::{PROXIES as ROCK_PROXIES, RockProxy, RockShape};
pub use roll::Roll;
pub use routine::{
    DAWN, DAY_TICKS, DUSK, Errand, HOUR_TICKS, NPC_WALK_UNITS_PER_TICK, NpcPlacement, NpcRoutine,
    ZONE_START_TIME, is_night, time_of_day,
};
pub use seat::{PostureView, SEAT_REACH_UNITS, Seat, SeatId, Sitting};
pub use signal::{SIGNAL_REACH_UNITS, SignalCall};
use sim_combat::class;
pub use sim_combat::class::{ClassChoice, PlayerClass, ResourceKind, Sex};
pub use sim_combat::melee::{
    CHAIN_LENGTH, CHAIN_WINDOW_TICKS, FOLLOW_UP_WINDOW_TICKS, FamilyMelee,
    LANDING_STRIKE_RECOVERY_PERCENT, LANDING_STRIKE_TOLERANCE_TICKS, MAX_CONTACT_TICKS,
    MAX_INTERVAL_TICKS, StrikeProfile, StrikeReward, StrikeVariant, WeaponFamily,
};
pub use sim_combat::unit;
pub use sim_foundation::{
    PlayerId, TICK_HZ, ZoneError, ZoneId, long_ticks_from_ms, physics, rng, subunits_per_tick,
    subunits_per_tick_squared, ticks_from_ms, trig, units_per_tick,
};
use sim_foundation::{entity, ramp, rock};
pub use sim_movement::{
    BACKPEDAL_SPEED_SUBUNITS_PER_TICK, BACKPEDAL_SPEED_UNITS_PER_SECOND,
    JUMP_VELOCITY_SUBUNITS_PER_TICK, JUMP_VELOCITY_UNITS_PER_SECOND, PLAYER_HALF_EXTENTS_UNITS,
    RUN_SPEED_SUBUNITS_PER_TICK, RUN_SPEED_UNITS_PER_SECOND, climb, roll, surface,
};
pub use snapshot::{
    CanonicalCreatureAbilities, CanonicalCreatureSnapshot, CanonicalPlayerAbilities,
    CanonicalPlayerCombat, CanonicalPlayerSnapshot, CanonicalZoneSnapshot, CreatureAi,
    CreatureLife, PlayerIntent, QueuedIntent, ThreatEntry,
};
pub use stance::{LANDING_RECOVERY_TICKS, LandingState, MeleeStance, OVERHEAD_LANDING_TICKS};
pub use strike::{
    STRIKE_ARC_HALF_WIDTH, STRIKE_SIDE_OFFSET, StrikeAim, StrikeHeight, StrikeSide,
    has_directed_mode,
};
pub use structure::{
    CanonicalPiece, PROJECTILE_PIECE_DAMAGE, PieceBox, PieceId, PieceMaterial, PieceStage,
    PieceView, REPAIR_REACH_UNITS, REPAIR_SPEEDUP_TICKS, StructurePiece, StructureSite,
};
pub use summoned::{
    MAX_SUMMONED_TERRAIN, MAX_VISIBLE_SUMMONED, PROJECTILE_SUMMONED_DAMAGE, SUMMON_DISTANCE_UNITS,
    SWING_SUMMONED_DAMAGE, SummonedTerrain, SummonedTerrainKind, SummonedView,
};
pub use supply::{
    INTERCEPT_RADIUS_UNITS, MAX_SUPPLY_MEMBERS, MAX_SUPPLY_PATROLS, MAX_SUPPLY_ROUTE_POINTS,
    SupplyMarch, SupplyPatrol, SupplyRun,
};
pub use surface::{SurfaceEffect, SurfaceKind, SurfacePatch};
pub use swing::{
    ClashKind, ClashRules, MAX_CLASH_RADIUS_UNITS, MAX_SWING_PHASE_TICKS, ProjectileInterception,
    SWING_HITS, Swing, SwingPhase, SwingTiming, TimedCombat,
};
pub use transfer::{CharacterTransfer, MAX_REMEMBERED_TRANSFERS, TransferId, TransferMotion};
pub use trap::{
    Blade, BladePose, CanonicalTrap, CanonicalTrapHit, MAX_ROCKS, MAX_TRAP_HITS, MAX_VISIBLE_TRAPS,
    RockDrop, RockPose, Rockfall, Trap, TrapEffect, TrapId, TrapKind, TrapRuntime, TrapTrigger,
    TrapView, TriggerVolume,
};
pub use vendor::{
    VENDOR_REACH_UNITS, VendorOffer, VendorStock, VendorStockError, VendorTradeError,
    fenford_trader_stock, sell_price, settle_purchase, settle_sale, smith_stock,
    starter_vendor_stock,
};
pub use water::{AirPocket, Breath, WaterChannel, WaterMode, WaterVolume};
pub use waystone::{
    BARROWDOWN, FENFORD, GREYHAVEN_OUTPOST, GREYHAVEN_ZONE, SEAM_TEST_EAST, SEAM_TEST_WEST,
    WAYSTONE_DISCOVERY_UNITS, WAYSTONES, WESTMERE_LODGE, WaystoneId, WaystoneSet, WaystoneSite,
    waystone, waystone_within_reach,
};
pub use zone::{MAX_PENDING_INTENTS, MAX_SEQUENCE_GAP, ZoneSimulation, ZoneTickWork};

/// Gravity of the authored zones and the test arenas: 9 m/s² down, in subunits per tick².
pub const STANDARD_GRAVITY: [i32; 3] = [0, subunits_per_tick_squared(-900), 0];

#[must_use]
pub const fn f32_units_per_tick(units_per_second: f32) -> f32 {
    units_per_second / TICK_HZ as f32
}

/// Units per tick² of an acceleration in units per second², for the `f32` projectile motion.
#[must_use]
pub const fn f32_units_per_tick_squared(units_per_second_squared: f32) -> f32 {
    units_per_second_squared / (TICK_HZ as f32 * TICK_HZ as f32)
}

pub const MAX_PLAYERS_PER_ZONE: usize = 512;
/// Inclusive XZ radius of player-scoped relevance (45 m).
pub const INTEREST_RADIUS_UNITS: i32 = 4_500;
/// An idle creature with no player within this XZ radius sleeps (#814): no AI and no physics
/// step. It reaches 15 m beyond relevance, so no player sees a creature fall asleep.
pub const CREATURE_WAKE_RADIUS_UNITS: i32 = INTEREST_RADIUS_UNITS + 1_500;
/// An awake idle creature decides on one tick in this many, staggered by creature ID, and
/// keeps its velocity in between (#814). Every other creature decides every tick.
pub const IDLE_DECISION_INTERVAL_TICKS: u16 = 10;
/// Deterministic relevance cap of one player projection: the viewer, its
/// target and the nearest units. `mmorpg-protocol` packs a projection into
/// its per-datagram byte budget in this priority order.
pub const MAX_VISIBLE_ENTITIES: usize = 64;

/// Player intent. The session runtime supplies identity and sequence separately.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ZoneCommand {
    /// Held movement relative to `facing`: `forward` and `strafe` are each in
    /// `-1..=1`; positive strafe is the character's right.
    Move {
        forward: i8,
        strafe: i8,
        facing: u16,
    },
    /// Edge-triggered jump, honoured on the next tick only when grounded.
    Jump,
    /// Edge-triggered dodge roll (#516), honoured on the next tick only when the player can
    /// roll: along the held movement, or straight back without any.
    Roll,
    /// Held swim ascent (`1`), descent (`-1`) or neither (`0`) (#274); it only acts while
    /// the player swims.
    Rise(i8),
    /// Selects a unit, or clears the selection with `None`.
    SelectTarget(Option<EntityRef>),
    /// Claim all remaining rewards from one fenced corpse death.
    Loot(LootClaim),
    /// Swings the weapon once toward `facing` (#517): the player turns to it and begins the
    /// strike its movement picks (#885); on its contact every attackable unit inside the
    /// strike's arc is hit, and a strike that covers nobody whiffs. A press while the previous
    /// strike recovers waits for it.
    Swing { facing: u16 },
    /// Returns a dead player to the graveyard with half health.
    ReleaseSpirit,
    /// Moves a quantity between the player's own bag slots on the next tick.
    MoveItem {
        source: u8,
        destination: u8,
        quantity: u16,
    },
    /// Uses a class ability (#517, #884): a hostile one aims at `target`, or the current
    /// selection with `None`, when it lies in the ability's aim cone, else at the unit in front;
    /// a friendly one lands on a selected friendly player or the caster; self-centred abilities
    /// ignore the target. Unknown IDs are refused in the tick, not here.
    UseAbility {
        ability: u32,
        target: Option<EntityRef>,
        /// Turns the player to this yaw first when given (#884), so the ability fires where the
        /// client aimed it rather than at a facing an earlier move left.
        facing: Option<u16>,
    },
    /// Releases the held input that pressed `ability` (#414): `retired` when
    /// `input-bindings` retired the hold (focus loss, a menu, a profile
    /// change) rather than the key going up. Held state like movement: only
    /// an experimental graph's hold-to-charge and chord triggers read it.
    ReleaseAbility { ability: u32, retired: bool },
    /// Executes `ability`'s directed mode (#154; only Heroic Strike has
    /// one): the player turns to `facing` and sweeps toward `aim`; the zone
    /// resolves which creature the sweep physically covers.
    DirectedStrike {
        ability: u32,
        facing: u16,
        aim: StrikeAim,
    },
    /// Holds the Warden's shield guard in a lane, or lowers it with `None`
    /// (#156). Held state like movement: the zone keeps it until the next
    /// guard command, death or a directed strike.
    Guard { lane: Option<StrikeHeight> },
    /// Explicit exploration (sheathed) or fight (drawn) mode (#802).
    SetWeaponDrawn(bool),
    /// Opens (`open`) or closes a door (#158). Asking for the state it
    /// already has changes nothing; reach and blocking bodies are checked in
    /// the tick.
    UseDoor { door: DoorId, open: bool },
    /// Sits on `seat`, or on the ground where the player stands with `None` (#619, Atlas
    /// 5.4.i619). The zone checks reach, whether it is held and the footing in the tick;
    /// moving, jumping, damage, casting or attacking ends it.
    Sit { seat: Option<SeatId> },
    /// Starts repairing a destructible piece (#305), or stops with `None`: held like a guard,
    /// free, and it speeds up the piece's timed regrowth while the player stands within
    /// reach. Any movement stops it.
    Repair { piece: Option<PieceId> },
    /// Stops the player's own cast or channel.
    CancelCast,
    /// Orders all of the player's minions at once (#295, Atlas 5.2.84); the zone checks the
    /// target or point in the tick.
    CommandMinions(MinionOrder),
    /// Chooses the class (0 Warden, 1 Ranger, 2 Arcanist, 3 Necromancer, 4 Rogue, 5 Cleric, 6 Druid) and sex
    /// (0 female, 1 male) once. Invalid or repeated choices are refused in the tick.
    ChooseClass { class: u8, sex: u8 },
    /// Equips the item in one of the player's own bag slots into its
    /// catalog equipment slot, swapping any item already there.
    EquipItem { bag_slot: u8 },
    /// Moves the item in an equipment slot (0 main hand … 5 feet) to the
    /// lowest empty bag slot.
    UnequipItem { equipment_slot: u8 },
    /// Buys `quantity` units of offer `offer` (stock index) from vendor `npc`.
    BuyItem {
        npc: NpcId,
        offer: u32,
        quantity: u16,
    },
    /// Sells `quantity` units from one of the player's own bag slots to vendor `npc`.
    SellItem {
        npc: NpcId,
        bag_slot: u8,
        quantity: u16,
    },
    /// Says or yells one validated line to the players in range.
    Chat {
        channel: ChatChannel,
        text: ChatText,
    },
    /// Performs one emote for the players in `/say` range (#69).
    Emote(Emote),
    /// Accepts `quest` from its giver `npc` (#25).
    AcceptQuest { npc: NpcId, quest: u32 },
    /// Turns `quest` in at its ender `npc`, taking reward `choice` (an
    /// index into the quest's choices; 0 when it offers none).
    CompleteQuest { npc: NpcId, quest: u32, choice: u8 },
    /// Drops `quest` from the log; its progress is lost.
    AbandonQuest { quest: u32 },
    /// Travels from the waystone within reach to the unlocked `waystone` (#145, Atlas
    /// 5.1.9); the zone checks reach, unlocks and combat in the tick.
    UseWaystone { waystone: WaystoneId },
    /// Tends or mends aid task `task` of the night raid's aftermath (#531, Atlas 5.1.51): an
    /// index into the raid site's authored tasks. The zone checks that it is open and within
    /// reach in the tick.
    RaidAid { task: u32 },
    /// One preparation step before a forewarned night raid (#532, Atlas 5.1.59–5.1.62); the zone
    /// checks the evening, reach and order in the tick.
    RaidPrepare { prep: RaidPrep },
    /// Gathers ore vein `node` (#917, an index into the content's veins): the zone checks that
    /// it is full and within reach in the tick, and spends it for everyone until it regrows.
    Gather { node: u32 },
    /// Inspects quest evidence `spot` (#1019, an [`EvidenceId`]): the zone checks reach and
    /// that an active quest still needs it in the tick, then completes those objectives.
    Inspect { spot: u32 },
    /// Performs the next open quest action at `npc` (#1019): uses a quest item on it, asks it for
    /// an interview or starts escorting it, checked in the tick.
    QuestAction { npc: NpcId },
    /// Decides the open decision of an active quest at `npc` (#1019): `option` is 1-based into
    /// the quest's [`Quest::decision`], checked in the tick and kept with the character.
    Decide { npc: NpcId, option: u8 },
    /// Works mechanism `mechanism` (#1019, a [`MechanismId`]): a seal's glyph or a ward's; the
    /// zone checks reach in the tick.
    Operate { mechanism: u32 },
    /// Lifts a load in both arms (#1077): from load source `source` (a [`LoadSpotId`]) an active
    /// quest still needs a delivery of, or with `None` the player's own set-down load. The zone
    /// checks empty hands, footing and reach in the tick.
    Lift { source: Option<LoadSpotId> },
    /// Sets the carried load down at the player's feet (#1077), on footing the zone checks in
    /// the tick.
    SetDown,
    /// Skins beast corpse `creature` (#917), which the player may loot: once per death, checked
    /// in the tick.
    Skin { creature: CreatureId },
    /// Answers the roll window of drop `drop` of `run` (#404, Atlas 5.1.34): stale, repeated
    /// and ineligible answers are refused in the tick, and a settled roll never changes.
    RollDepthsDrop {
        run: u64,
        drop: u8,
        choice: RollChoice,
    },
    /// Takes the held drop `drop` of `run`, which everyone passed on (Atlas 5.1.83): one
    /// claimant, checked in the tick.
    ClaimDepthsDrop { run: u64, drop: u8 },
}
