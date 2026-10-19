#![forbid(unsafe_code)]

//! Deterministic zone-local gameplay: movement, units, combat, creature AI
//! and the interest/visibility policy of player projections.
//!
//! [`ZoneSimulation`] advances in the fixed tick order documented on
//! [`ZoneSimulation::advance_tick`]. Physics integration and collision stay in
//! Rapier, embedded in [`physics`] (#271); wire formats live in `mmorpg-protocol`.

pub mod ability;
mod ai;
mod areas;
mod aura;
mod breakable;
mod casting;
mod character;
mod chat;
mod class;
mod combat;
mod combo;
mod content;
mod corpse_loot;
mod creature;
mod door;
mod entity;
mod equipment;
mod events;
mod forced;
#[doc(hidden)]
pub mod golden;
mod guard;
mod interest;
mod inventory;
mod loot;
pub mod physics;
#[cfg(feature = "physics-ab")]
pub mod physics_ab;
mod platform;
mod progression;
mod projectile;
mod projection;
mod quest;
pub mod rng;
mod routine;
mod snapshot;
mod stance;
mod strike;
pub mod trig;
pub mod unit;
mod vendor;
mod zone;

pub use ability::{
    ABILITY_CATALOG, ABILITY_CATALOG_REVISION, Ability, AbilityEffect, AbilityId, AbilityRange,
    AbilityUser, AuraKind, AuraSpec, CastTime, GLOBAL_COOLDOWN_TICKS, MAX_AURAS, MAX_COOLDOWNS,
    ability_by_id,
};
pub use areas::{Area, AreaId, MAX_AREA_NAME_BYTES, MAX_ZONE_AREAS, ZoneAreas};
pub use aura::{Aura, CastState, Cooldown};
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
pub use combo::{COMBO_IMPACT_TICKS, COMBO_WINDOW_TICKS, ComboOpening, ComboPhase};
pub use content::greyhaven_vale::{self, greyhaven_vale_definition};
pub use content::{
    CreatureBehaviour, CreatureFamily, CreatureSpawn, CreatureTemplate,
    LARGE_CREATURE_HALF_WIDTH_UNITS, MAX_CONTENT_COORDINATE_UNITS, MAX_CREATURE_HALF_EXTENT_UNITS,
    MAX_CREATURE_SPAWNS, MAX_CREATURE_TEMPLATES, MAX_NPCS, MAX_STATIC_COLLIDERS,
    MAX_TERRAIN_HEIGHT_UNITS, MAX_UNIT_NAME_BYTES, MAX_WANDER_RADIUS_UNITS, Npc, NpcRole, PropKind,
    SpawnGrid, StaticCollider, Terrain, UNITS_PER_METRE, XzBounds, ZoneContent, ZoneDefinition,
};
pub use corpse_loot::{LOOT_REACH_UNITS, LootClaim, LootView};
pub use creature::{
    AGGRO_MAX_RADIUS_UNITS, AGGRO_MIN_RADIUS_UNITS, AGGRO_PER_LEVEL_UNITS, AGGRO_RADIUS_UNITS,
    ARRIVAL_RADIUS_UNITS, ASSIST_RADIUS_UNITS, CREATURE_EVADE_SPEED_UNITS_PER_TICK,
    CREATURE_RUN_SPEED_UNITS_PER_TICK, CREATURE_WALK_SPEED_UNITS_PER_TICK, EVADE_TIMEOUT_TICKS,
    LEASH_RADIUS_UNITS, MAX_THREAT_ENTRIES, WANDER_WAIT_TICKS, WANDER_WALK_TIMEOUT_TICKS,
};
pub use door::{
    CanonicalDoor, DOOR_COOLDOWN_TICKS, DOOR_REACH_UNITS, Door, DoorBox, DoorId, DoorView,
    MAX_DOORS, NPC_SIGHT_HEIGHT_UNITS,
};
pub use entity::{CreatureId, CreatureTemplateId, EntityKind, EntityRef, NpcId};
pub use equipment::{
    EQUIPMENT_SLOTS, Equipment, EquipmentSlot, HEALTH_PER_STAMINA, ItemStats, MAX_EQUIPPED_STAMINA,
    PRIMARY_STAT_PER_DAMAGE, StatTotals,
};
pub use events::{ErrorCode, MAX_EVENTS_PER_PLAYER, ZoneEvent};
pub use forced::{
    CHARGE_MAX_TICKS, CHARGE_SPEED_UNITS, FORCED_IMMUNITY_TICKS, ForcedMove, ForcedState,
    KNOCKBACK_TICKS, SHOVE_UNITS,
};
pub use guard::{
    GUARD_ARC_HALF_WIDTH, GUARD_ARC_OFFSET, GUARD_BREAK_IMPACT, GUARD_BREAK_TICKS,
    GUARD_SPEED_PERCENT, GuardState, STRIKE_RECOVERY_TICKS, lane_of,
};
pub use interest::{InterestMaintenanceStats, InterestQueryStats, PlayerProjection};
pub use inventory::{
    INVENTORY_RESEND_TICKS, INVENTORY_SLOTS, ITEM_CATALOG, ITEM_CATALOG_REVISION, Inventory,
    InventoryError, ItemId, ItemStack, ItemTemplate, item_template,
};
pub use loot::{
    LOOT_CATALOG_REVISION, LootOutcome, LootRewards, LootRolls, LootSettlementError, LootTable,
    LootTableError, MAX_LOOT_OUTCOMES, loot_table, settle_loot,
};
pub use platform::{MAX_PLATFORMS, Platform, PlatformId, PlatformPhase};
pub use progression::{MAX_PLAYER_LEVEL, experience_to_next_level, kill_experience};
pub use projectile::{
    ARROW_SPEED_UNITS, BOLT_SPEED_UNITS, Flight, MAX_PROJECTILES, MAX_VISIBLE_PROJECTILES,
    Projectile, ProjectileKind, ProjectileView, STUCK_IN_UNIT_TICKS, STUCK_TICKS,
};
pub use projection::{
    AuraView, CastView, EntityFlags, EntitySnapshot, GuardView, ResourceView, TargetDetail,
    ViewerState, ZoneSnapshot,
};
pub use quest::{
    MAX_QUEST_EXPERIENCE, MAX_QUEST_LOG, MAX_QUEST_NAME_BYTES, MAX_QUEST_NPCS,
    MAX_QUEST_OBJECTIVES, MAX_QUEST_TEXT_BYTES, MAX_QUESTS, MAX_REWARD_CHOICES, NpcMarker,
    QUEST_CATALOG_REVISION, QUEST_REACH_UNITS, Quest, QuestContentError, QuestEntry, QuestId,
    QuestLog, QuestMarker, QuestObjective, QuestRewards, QuestSheet,
};
pub use routine::{
    DAWN, DAY_TICKS, DUSK, HOUR_TICKS, MAX_ROUTINE_WAYPOINTS, NPC_WALK_UNITS_PER_TICK,
    NpcPlacement, NpcRoutine, ZONE_START_TIME, is_night, time_of_day,
};
pub use snapshot::{
    CanonicalCreatureAbilities, CanonicalCreatureSnapshot, CanonicalPlayerAbilities,
    CanonicalPlayerCombat, CanonicalPlayerSnapshot, CanonicalZoneSnapshot, CreatureAi,
    CreatureLife, PlayerIntent, ThreatEntry,
};
pub use stance::{LANDING_RECOVERY_TICKS, LandingState, MeleeStance, OVERHEAD_LANDING_TICKS};
pub use strike::{STRIKE_ARC_HALF_WIDTH, StrikeAim, StrikeHeight, StrikeSide, has_directed_mode};
pub use vendor::{
    MAX_VENDOR_OFFERS, VENDOR_CATALOG_REVISION, VENDOR_REACH_UNITS, VendorOffer, VendorStock,
    VendorStockError, VendorTradeError, sell_price, settle_purchase, settle_sale,
    starter_vendor_stock,
};
pub use zone::{MAX_PENDING_INTENTS, ZoneSimulation, ZoneTickWork};

use std::error::Error;
use std::fmt;

pub type PlayerId = u32;

pub const TICK_HZ: u16 = 30;
pub const MAX_PLAYERS_PER_ZONE: usize = 512;
/// Core schema of canonical and player-visible snapshots.
pub const SNAPSHOT_SCHEMA_VERSION: u16 = 25;
/// Inclusive XZ radius of player-scoped relevance (45 m).
pub const INTEREST_RADIUS_UNITS: i32 = 4_500;
/// Deterministic relevance cap of one player projection: the viewer, its
/// target and the nearest units. `mmorpg-protocol` packs a projection into
/// its per-datagram byte budget in this priority order.
pub const MAX_VISIBLE_ENTITIES: usize = 64;

/// Character collision box: 0.6 m × 1.8 m × 0.6 m, shared with clients.
pub const PLAYER_HALF_EXTENTS_UNITS: [i32; 3] = [30, 90, 30];
/// Horizontal speed for forward, strafe and forward-diagonal intent (6.3 m/s).
pub const RUN_SPEED_UNITS_PER_TICK: i32 = 21;
/// Horizontal speed whenever intent has a backward component.
pub const BACKPEDAL_SPEED_UNITS_PER_TICK: i32 = 13;
/// Upward velocity a grounded jump sets before physics applies gravity.
pub const JUMP_VELOCITY_UNITS_PER_TICK: i32 = 16;

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
    /// Selects a unit, or clears the selection with `None`.
    SelectTarget(Option<EntityRef>),
    /// Claim all remaining rewards from one fenced corpse death.
    Loot(LootClaim),
    /// Starts auto-attacking the selected target.
    StartAttack,
    StopAttack,
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
    Guard {
        lane: Option<StrikeHeight>,
    },
    /// Opens (`open`) or closes a door (#158). Asking for the state it
    /// already has changes nothing; reach and blocking bodies are checked in
    /// the tick.
    UseDoor {
        door: DoorId,
        open: bool,
    },
    /// Stops the player's own cast or channel.
    CancelCast,
    /// Chooses the class (0 Warden, 1 Ranger, 2 Arcanist) and sex (0 female,
    /// 1 male) once. Invalid or repeated choices are refused in the tick.
    ChooseClass {
        class: u8,
        sex: u8,
    },
    /// Equips the item in one of the player's own bag slots into its
    /// catalog equipment slot, swapping any item already there.
    EquipItem {
        bag_slot: u8,
    },
    /// Moves the item in an equipment slot (0 main hand … 5 feet) to the
    /// lowest empty bag slot.
    UnequipItem {
        equipment_slot: u8,
    },
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
    AcceptQuest {
        npc: NpcId,
        quest: u8,
    },
    /// Turns `quest` in at its ender `npc`, taking reward `choice` (an
    /// index into the quest's choices; 0 when it offers none).
    CompleteQuest {
        npc: NpcId,
        quest: u8,
        choice: u8,
    },
    /// Drops `quest` from the log; its progress is lost.
    AbandonQuest {
        quest: u8,
    },
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
