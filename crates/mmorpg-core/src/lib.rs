#![forbid(unsafe_code)]

//! Deterministic zone-local gameplay: movement, units, combat, creature AI
//! and the interest/visibility policy of player projections.
//!
//! [`ZoneSimulation`] advances in the fixed tick order documented on
//! [`ZoneSimulation::advance_tick`]. Physics integration and collision stay in
//! Rapier, embedded in [`physics`] (#271); wire formats live in `mmorpg-protocol`.

pub mod ability;
pub mod action;
mod ai;
mod areas;
mod aura;
mod boss;
mod breakable;
mod casting;
mod character;
mod chat;
mod class;
pub mod climb;
mod combat;
mod combo;
mod content;
mod corpse_loot;
mod corridor;
mod creature;
mod door;
mod entity;
mod ephemeral;
mod equipment;
mod events;
mod forced;
#[doc(hidden)]
pub mod golden;
mod guard;
mod home;
mod interest;
mod inventory;
mod loot;
mod minion;
mod movable;
mod nav;
pub mod physics;
#[cfg(feature = "physics-ab")]
pub mod physics_ab;
mod platform;
mod progression;
mod projectile;
mod projection;
mod pvp;
mod quest;
pub mod raid;
mod ramp;
mod rewards;
pub mod rng;
pub mod roll;
mod routine;
mod shapes;
mod snapshot;
mod stance;
mod strike;
mod structure;
mod transfer;
pub mod trig;
pub mod unit;
mod vendor;
pub mod water;
mod waystone;
mod zone;

pub use ability::{
    ABILITY_CATALOG, Ability, AbilityEffect, AbilityId, AbilityRange, AbilityUser, AuraKind,
    AuraSpec, CastTime, GLOBAL_COOLDOWN_TICKS, MAX_AURAS, MAX_COOLDOWNS, Shot, ability_by_id,
};
pub use areas::{Area, AreaId, MAX_AREA_NAME_BYTES, MAX_ZONE_AREAS, ZoneAreas};
pub use aura::{Aura, CastState, Cooldown};
pub use boss::{BossEncounter, BossRules, MAX_ENCOUNTER_PLAYERS, MAX_SCALING_PARTICIPANTS};
pub use breakable::{
    DEBRIS_TICKS, Debris, DebrisPiece, DebrisView, EXPLOSION_DAMAGE, EXPLOSION_KNOCKBACK_UNITS,
    EXPLOSION_RADIUS_UNITS, MAX_DEBRIS, MAX_VISIBLE_DEBRIS, debris_pieces,
};
pub use character::{
    CHARACTER_RECORD_SCHEMA_VERSION, CharacterId, CharacterLocation, CharacterRecord,
    MAX_CHARACTER_ID_BYTES,
};
pub use chat::{
    CHAT_INTERVAL_TICKS, ChatChannel, ChatLine, ChatMessage, ChatText, Emote, MAX_CHAT_BYTES,
    MAX_CHAT_PER_TICK, SAY_RANGE_UNITS, YELL_RANGE_UNITS,
};
pub use class::{ClassChoice, PlayerClass, ResourceKind, Sex};
pub use climb::{ClimbSurfaces, Grip, Traversal, TraversalView};
pub use combo::{COMBO_IMPACT_TICKS, COMBO_WINDOW_TICKS, ComboOpening, ComboPhase};
pub use content::greyhaven_vale::{self, greyhaven_vale_definition};
pub use content::seam_test::{self, content_for_zone};
pub use content::{
    CharacterPersistence, CreatureBehaviour, CreatureFamily, CreatureSpawn, CreatureTemplate,
    LARGE_CREATURE_HALF_WIDTH_UNITS, MAX_CONTENT_COORDINATE_UNITS, MAX_CREATURE_HALF_EXTENT_UNITS,
    MAX_CREATURE_SPAWNS, MAX_CREATURE_TEMPLATES, MAX_NPCS, MAX_STATIC_COLLIDERS,
    MAX_TERRAIN_HEIGHT_UNITS, MAX_UNIT_NAME_BYTES, MAX_WANDER_RADIUS_UNITS, Npc, NpcRole,
    PlayerCombatPolicy, PropKind, SpawnGrid, StaticCollider, Terrain, UNITS_PER_METRE, XzBounds,
    ZoneContent, ZoneDefinition,
};
pub use corpse_loot::{LOOT_REACH_UNITS, LootClaim, LootView};
pub use corridor::{
    CORRIDOR_TURN_BACK_UNITS, HandoffCorridor, MAX_HANDOFF_CORRIDORS, SeamCrossing,
};
pub use creature::{
    AGGRO_MAX_RADIUS_UNITS, AGGRO_MIN_RADIUS_UNITS, AGGRO_PER_LEVEL_UNITS, AGGRO_RADIUS_UNITS,
    ARRIVAL_RADIUS_UNITS, ASSIST_RADIUS_UNITS, CREATURE_EVADE_SPEED_SUBUNITS_PER_TICK,
    CREATURE_RUN_SPEED_SUBUNITS_PER_TICK, CREATURE_WALK_SPEED_SUBUNITS_PER_TICK,
    EVADE_TIMEOUT_TICKS, LEASH_RADIUS_UNITS, MAX_THREAT_ENTRIES, WANDER_WAIT_TICKS,
    WANDER_WALK_TIMEOUT_TICKS,
};
pub use door::{
    CanonicalDoor, DOOR_COOLDOWN_TICKS, DOOR_REACH_UNITS, Door, DoorBox, DoorId, DoorView,
    MAX_DOORS, NPC_SIGHT_HEIGHT_UNITS,
};
pub use entity::{CreatureId, CreatureTemplateId, EntityKind, EntityRef, MinionId, NpcId};
pub use ephemeral::EphemeralCharacterBuild;
pub use equipment::{
    EQUIPMENT_SLOTS, Equipment, EquipmentSlot, HEALTH_PER_STAMINA, ItemStats, MAX_EQUIPPED_STAMINA,
    PRIMARY_STAT_PER_DAMAGE, StatTotals,
};
pub use events::{ErrorCode, MAX_EVENTS_PER_PLAYER, ZoneEvent};
pub use forced::{
    CHARGE_MAX_TICKS, CHARGE_SPEED_SUBUNITS_PER_TICK, FORCED_IMMUNITY_TICKS, ForcedMove,
    ForcedState, KNOCKBACK_TICKS, SHOVE_UNITS,
};
pub use guard::{
    GUARD_ARC_HALF_WIDTH, GUARD_ARC_OFFSET, GUARD_BREAK_IMPACT, GUARD_BREAK_TICKS,
    GUARD_SPEED_PERCENT, GuardState, STRIKE_RECOVERY_TICKS, lane_of,
};
pub use home::{
    MAX_WAKE_WAYPOINTS, NpcWake, PRIVATE_DOOR_CLOSE_TICKS, PrivateHome, WAKE_IDLE_TICKS,
};
pub use interest::{InterestMaintenanceStats, InterestQueryStats, PlayerProjection};
pub use inventory::{
    INVENTORY_RESEND_TICKS, INVENTORY_SLOTS, ITEM_CATALOG, Inventory, InventoryError, ItemId,
    ItemStack, ItemTemplate, item_template,
};
pub use loot::{
    LootOutcome, LootRewards, LootRolls, LootSettlementError, LootTable, LootTableError,
    MAX_LOOT_OUTCOMES, loot_table, settle_loot,
};
pub use minion::{
    COMPANION_HALF_EXTENTS_UNITS, CanonicalMinion, DEFEND_RADIUS_UNITS, ENGAGE_RADIUS_UNITS,
    FORMATION_RADIUS_UNITS, MAX_MINIONS, MAX_MINIONS_PER_SUMMONER, MINION_HALF_EXTENTS_UNITS,
    Minion, MinionOrder, ORDER_RANGE_UNITS, is_companion, minion_ability, minion_half_extents,
};
pub use nav::NAV_CELL_UNITS;
pub use platform::{MAX_PLATFORMS, Platform, PlatformId, PlatformPhase};
pub use progression::{MAX_PLAYER_LEVEL, experience_to_next_level, kill_experience};
pub use projectile::{
    ARROW_SPEED_UNITS, BOLT_SPEED_UNITS, BOMB_FUSE_TICKS, BOMB_RESTITUTION, BURST_TICKS, Flight,
    KNIFE_SPEED_UNITS, MAX_PROJECTILES, MAX_VISIBLE_PROJECTILES, MISSILE_FLIGHT_HEIGHT_UNITS,
    MISSILE_SPEED_UNITS, PIERCE_DAMAGE_PERCENT, PIERCE_HITS, Projectile, ProjectileKind,
    ProjectileView, RICOCHET_BOUNCES, STUCK_IN_UNIT_TICKS, STUCK_TICKS, THROW_RANGE_UNITS,
    THROW_RISE_UNITS, THROW_SPEED_UNITS, THROWN_FLIGHT_TICKS,
};
pub use projection::{
    AuraView, CastView, EntityFlags, EntitySnapshot, GuardView, ResourceView, TargetDetail,
    ViewerState, ZoneSnapshot,
};
pub use quest::{
    MAX_QUEST_EXPERIENCE, MAX_QUEST_LOG, MAX_QUEST_NAME_BYTES, MAX_QUEST_NPCS,
    MAX_QUEST_OBJECTIVES, MAX_QUEST_TEXT_BYTES, MAX_QUESTS, MAX_REWARD_CHOICES, NpcMarker,
    QUEST_REACH_UNITS, Quest, QuestContentError, QuestEntry, QuestId, QuestLog, QuestMarker,
    QuestObjective, QuestRewards, QuestSheet,
};
pub use raid::{
    RaidApproach, RaidIncident, RaidLeg, RaidMember, RaidOutcome, RaidResolution, RaidSite,
    RaidState, RaidView,
};
pub use ramp::{MAX_RAMPS, Ramp, RampRise};
pub use rewards::{RewardGrant, RewardHistory, RewardKey};
pub use roll::Roll;
pub use routine::{
    DAWN, DAY_TICKS, DUSK, HOUR_TICKS, MAX_ROUTINE_WAYPOINTS, NPC_WALK_UNITS_PER_TICK,
    NpcPlacement, NpcRoutine, ZONE_START_TIME, is_night, time_of_day,
};
pub use snapshot::{
    CanonicalCreatureAbilities, CanonicalCreatureSnapshot, CanonicalPlayerAbilities,
    CanonicalPlayerCombat, CanonicalPlayerSnapshot, CanonicalZoneSnapshot, CreatureAi,
    CreatureLife, PlayerIntent, QueuedIntent, ThreatEntry,
};
pub use stance::{LANDING_RECOVERY_TICKS, LandingState, MeleeStance, OVERHEAD_LANDING_TICKS};
pub use strike::{STRIKE_ARC_HALF_WIDTH, StrikeAim, StrikeHeight, StrikeSide, has_directed_mode};
pub use structure::{
    CanonicalPiece, MAX_PIECES, MAX_SITES, PROJECTILE_PIECE_DAMAGE, PieceBox, PieceId, PieceStage,
    PieceView, REPAIR_REACH_UNITS, REPAIR_SPEEDUP_TICKS, StructurePiece, StructureSite,
};
pub use transfer::{CharacterTransfer, MAX_REMEMBERED_TRANSFERS, TransferId, TransferMotion};
pub use vendor::{
    MAX_VENDOR_OFFERS, VENDOR_REACH_UNITS, VendorOffer, VendorStock, VendorStockError,
    VendorTradeError, sell_price, settle_purchase, settle_sale, starter_vendor_stock,
};
pub use water::{AirPocket, Breath, WaterMode, WaterVolume};
pub use waystone::{
    GREYHAVEN_OUTPOST, GREYHAVEN_ZONE, MAX_WAYSTONES, SEAM_TEST_EAST, SEAM_TEST_WEST,
    WAYSTONE_DISCOVERY_UNITS, WAYSTONES, WaystoneId, WaystoneSet, WaystoneSite, waystone,
    waystone_within_reach,
};
pub use zone::{MAX_PENDING_INTENTS, ZoneSimulation, ZoneTickWork};

use std::error::Error;
use std::fmt;

pub type PlayerId = u32;

/// The authoritative simulation rate. Durations are authored in milliseconds and
/// speeds in units per second; the helpers below are the only place they become
/// ticks, and the handshake and both clients read this constant.
pub const TICK_HZ: u16 = 60;

/// Whole ticks in `ms` milliseconds at [`TICK_HZ`], rounded to the nearest tick
/// (halves up): 1 tick lasts 16⅔ ms at 60 Hz, so a duration that is not a
/// multiple of 50 ms is authored as its nearest whole millisecond. A nonzero
/// duration shorter than half a tick, or one longer than `u16::MAX` ticks, fails
/// to compile in const contexts.
#[must_use]
pub const fn ticks_from_ms(ms: u32) -> u16 {
    let ticks = long_ticks_from_ms(ms);
    assert!(ticks <= u16::MAX as u32, "duration is too long");
    ticks as u16
}

/// [`ticks_from_ms`] for the long `u32` timers (zone day, respawns, corpses).
#[must_use]
pub const fn long_ticks_from_ms(ms: u32) -> u32 {
    let ticks = (ms as u64 * TICK_HZ as u64 + 500) / 1_000;
    assert!(ms == 0 || ticks > 0, "duration is shorter than half a tick");
    assert!(ticks <= u32::MAX as u64, "duration is too long");
    ticks as u32
}

/// Units per tick of a speed authored in units per second. A speed that is not
/// a whole number of units per tick fails to compile in const contexts.
#[must_use]
pub const fn units_per_tick(units_per_second: i32) -> i32 {
    assert!(
        units_per_second % TICK_HZ as i32 == 0,
        "speed is not a whole number of units per tick"
    );
    units_per_second / TICK_HZ as i32
}

/// Subunits ([`physics::SUBUNITS_PER_UNIT`]) per tick² of an acceleration authored in units
/// per second². One that is not a whole number of subunits per tick² fails to compile in const
/// contexts.
#[must_use]
pub const fn subunits_per_tick_squared(units_per_second_squared: i32) -> i32 {
    let subunits = units_per_second_squared * physics::SUBUNITS_PER_UNIT;
    let ticks_squared = TICK_HZ as i32 * TICK_HZ as i32;
    assert!(
        subunits % ticks_squared == 0,
        "acceleration is not a whole number of subunits per tick squared"
    );
    subunits / ticks_squared
}

/// Gravity of the authored zones and the test arenas: 9 m/s² down, in subunits per tick².
pub const STANDARD_GRAVITY: [i32; 3] = [0, subunits_per_tick_squared(-900), 0];

/// Subunits ([`physics::SUBUNITS_PER_UNIT`]) per tick of a speed authored in units per second:
/// the velocities the zone hands physics. One that is not a whole number of subunits per tick
/// fails to compile in const contexts.
#[must_use]
pub const fn subunits_per_tick(units_per_second: i32) -> i32 {
    let subunits = units_per_second * physics::SUBUNITS_PER_UNIT;
    assert!(
        subunits % TICK_HZ as i32 == 0,
        "speed is not a whole number of subunits per tick"
    );
    subunits / TICK_HZ as i32
}

/// [`units_per_tick`] for the `f32` projectile motion, which needs no whole units.
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
/// Deterministic relevance cap of one player projection: the viewer, its
/// target and the nearest units. `mmorpg-protocol` packs a projection into
/// its per-datagram byte budget in this priority order.
pub const MAX_VISIBLE_ENTITIES: usize = 64;

/// Character collision box: 0.6 m × 1.8 m × 0.6 m, shared with clients.
pub const PLAYER_HALF_EXTENTS_UNITS: [i32; 3] = [30, 90, 30];
/// Horizontal speed for forward, strafe and forward-diagonal intent (6.3 m/s).
pub const RUN_SPEED_UNITS_PER_SECOND: i32 = 630;
pub const RUN_SPEED_SUBUNITS_PER_TICK: i32 = subunits_per_tick(RUN_SPEED_UNITS_PER_SECOND);
/// Horizontal speed whenever intent has a backward component (3.9 m/s).
pub const BACKPEDAL_SPEED_UNITS_PER_SECOND: i32 = 390;
pub const BACKPEDAL_SPEED_SUBUNITS_PER_TICK: i32 =
    subunits_per_tick(BACKPEDAL_SPEED_UNITS_PER_SECOND);
/// Upward velocity a grounded jump sets before physics applies gravity (4.8 m/s).
pub const JUMP_VELOCITY_UNITS_PER_SECOND: i32 = 480;
pub const JUMP_VELOCITY_SUBUNITS_PER_TICK: i32 = subunits_per_tick(JUMP_VELOCITY_UNITS_PER_SECOND);
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ZoneId(u32);

impl ZoneId {
    #[must_use]
    pub const fn new(value: u32) -> Self {
        Self(value)
    }

    #[must_use]
    pub const fn get(self) -> u32 {
        self.0
    }
}

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
    /// Swings the weapon once toward `facing` (#517): the player turns to it and every
    /// attackable unit inside the melee arc is hit; a swing that covers nobody whiffs. A press
    /// while the previous swing recovers waits for it.
    Swing { facing: u16 },
    /// Returns a dead player to the graveyard with half health.
    ReleaseSpirit,
    /// Moves a quantity between the player's own bag slots on the next tick.
    MoveItem {
        source: u8,
        destination: u8,
        quantity: u16,
    },
    /// Uses a class ability at `target`, or at the current selection with
    /// `None`; self-centred abilities ignore the target. Unknown IDs are
    /// refused in the tick, not here.
    UseAbility {
        ability: u8,
        target: Option<EntityRef>,
    },
    /// Releases the held input that pressed `ability` (#414): `retired` when
    /// `input-bindings` retired the hold (focus loss, a menu, a profile
    /// change) rather than the key going up. Held state like movement: only
    /// an experimental graph's hold-to-charge and chord triggers read it.
    ReleaseAbility { ability: u8, retired: bool },
    /// Executes `ability`'s directed mode (#154; only Heroic Strike has
    /// one): the player turns to `facing` and sweeps toward `aim`; the zone
    /// resolves which creature the sweep physically covers.
    DirectedStrike {
        ability: u8,
        facing: u16,
        aim: StrikeAim,
    },
    /// Holds the Warden's shield guard in a lane, or lowers it with `None`
    /// (#156). Held state like movement: the zone keeps it until the next
    /// guard command, death or a directed strike.
    Guard { lane: Option<StrikeHeight> },
    /// Opens (`open`) or closes a door (#158). Asking for the state it
    /// already has changes nothing; reach and blocking bodies are checked in
    /// the tick.
    UseDoor { door: DoorId, open: bool },
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
        offer: u8,
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
    AcceptQuest { npc: NpcId, quest: u8 },
    /// Turns `quest` in at its ender `npc`, taking reward `choice` (an
    /// index into the quest's choices; 0 when it offers none).
    CompleteQuest { npc: NpcId, quest: u8, choice: u8 },
    /// Drops `quest` from the log; its progress is lost.
    AbandonQuest { quest: u8 },
    /// Travels from the waystone within reach to the unlocked `waystone` (#145, Atlas
    /// 5.1.9); the zone checks reach, unlocks and combat in the tick.
    UseWaystone { waystone: WaystoneId },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ZoneError {
    message: String,
}

impl ZoneError {
    #[must_use]
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }

    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }
}

impl fmt::Display for ZoneError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl Error for ZoneError {}
