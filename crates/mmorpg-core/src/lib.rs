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
mod boulder;
mod breakable;
mod casting;
mod character;
mod chat;
mod combat;
mod combo;
mod content;
mod corpse_loot;
mod corridor;
mod creature;
mod depths;
mod depths_boss;
mod door;
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
mod mobility;
mod movable;
mod nav;
mod party;
#[cfg(feature = "physics-ab")]
pub mod physics_ab;
mod physique;
mod platform;
mod progression;
mod projectile;
mod projection;
mod pvp;
mod quest;
pub mod raid;
mod rewards;
mod routine;
mod seat;
mod shapes;
mod snapshot;
mod stance;
mod strike;
mod structure;
mod summoned;
mod transfer;
pub mod trap;
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
pub use boulder::{CanonicalBoulder, IMPACT_SPEED, RETURN_TICKS, WALK_PUSH_SPEED};
pub use breakable::{
    DEBRIS_TICKS, Debris, DebrisPiece, DebrisView, EXPLOSION_DAMAGE, EXPLOSION_KNOCKBACK_UNITS,
    EXPLOSION_RADIUS_UNITS, MAX_DEBRIS, MAX_VISIBLE_DEBRIS, debris_pieces,
};
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
pub use corpse_loot::{LOOT_REACH_UNITS, LootClaim, LootView};
pub use corridor::{CORRIDOR_TURN_BACK_UNITS, Departure, HandoffCorridor, MAX_HANDOFF_CORRIDORS};
pub use creature::{
    AGGRO_MAX_RADIUS_UNITS, AGGRO_MIN_RADIUS_UNITS, AGGRO_PER_LEVEL_UNITS, AGGRO_RADIUS_UNITS,
    ARRIVAL_RADIUS_UNITS, ASSIST_RADIUS_UNITS, CREATURE_EVADE_SPEED_SUBUNITS_PER_TICK,
    CREATURE_RUN_SPEED_SUBUNITS_PER_TICK, CREATURE_WALK_SPEED_SUBUNITS_PER_TICK,
    EVADE_TIMEOUT_TICKS, LEASH_RADIUS_UNITS, MAX_THREAT_ENTRIES, WANDER_WAIT_TICKS,
    WANDER_WALK_TIMEOUT_TICKS,
};
pub use depths::{
    DepthsChamber, DepthsRun, DepthsRunStart, DepthsSite, DepthsSpawn, DepthsThreshold,
    MAX_CHAMBERS, ThresholdRequest, WIPE_DELAY_TICKS, depths_arrival,
};
pub use depths_boss::{
    BossAttempt, BossDrop, BossOutcome, BossProgress, BossReport, DepthsBoss, DepthsDropView,
    DepthsLootView, DropState, DropView, MAX_BOSS_DROPS, RollChoice, RollWin, equippable,
    pick_winner, resolve_roll, roll_for,
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
    WAKE_RISE_TICKS,
};
pub use interest::{InterestMaintenanceStats, InterestQueryStats, PlayerProjection};
pub use inventory::{
    INVENTORY_SLOTS, ITEM_CATALOG, Inventory, InventoryError, ItemId, ItemStack, ItemTemplate,
    item_template,
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
pub use party::{
    MAX_PARTY_MEMBERS, MAX_PARTY_NAME_BYTES, PartyInvite, PartyMember, PartyRefusal, PartyRoster,
    PartyState, valid_name as valid_party_name,
};
pub use physique::{
    InvalidPhysique, MAX_PHYSIQUE_MODIFIER_BP, PHYSIQUE_ATTRIBUTES, PHYSIQUE_COEFFICIENTS,
    PHYSIQUE_NEUTRAL, PHYSIQUE_PARAMS, PHYSIQUE_SCALE, PHYSIQUE_STAT_PARAMS, Physique,
    PhysiqueAttribute, PhysiqueParam, PhysiquePreset,
};
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
    AidKind, AidTask, RaidApproach, RaidIncident, RaidLeg, RaidMember, RaidNotice, RaidOutcome,
    RaidPrep, RaidResolution, RaidSite, RaidState, RaidView,
};
pub use ramp::{MAX_RAMPS, Ramp, RampRise};
pub use rewards::{RewardGrant, RewardHistory, RewardKey};
pub use rock::{PROXIES as ROCK_PROXIES, RockProxy, RockShape};
pub use roll::Roll;
pub use routine::{
    DAWN, DAY_TICKS, DUSK, HOUR_TICKS, MAX_ROUTINE_WAYPOINTS, NPC_WALK_UNITS_PER_TICK,
    NpcPlacement, NpcRoutine, ZONE_START_TIME, is_night, time_of_day,
};
pub use seat::{MAX_SEATS, PostureView, SEAT_REACH_UNITS, Seat, SeatId, Sitting};
use sim_combat::class;
pub use sim_combat::class::{ClassChoice, PlayerClass, ResourceKind, Sex};
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
pub use strike::{STRIKE_ARC_HALF_WIDTH, StrikeAim, StrikeHeight, StrikeSide, has_directed_mode};
pub use structure::{
    CanonicalPiece, MAX_PIECES, MAX_SITES, PROJECTILE_PIECE_DAMAGE, PieceBox, PieceId, PieceStage,
    PieceView, REPAIR_REACH_UNITS, REPAIR_SPEEDUP_TICKS, StructurePiece, StructureSite,
};
pub use summoned::{
    MAX_SUMMONED_TERRAIN, MAX_VISIBLE_SUMMONED, PROJECTILE_SUMMONED_DAMAGE, SUMMON_DISTANCE_UNITS,
    SWING_SUMMONED_DAMAGE, SummonedTerrain, SummonedTerrainKind, SummonedView,
};
pub use surface::{SurfaceEffect, SurfaceKind, SurfacePatch};
pub use transfer::{CharacterTransfer, MAX_REMEMBERED_TRANSFERS, TransferId, TransferMotion};
pub use trap::{
    Blade, BladePose, CanonicalTrap, CanonicalTrapHit, MAX_ROCKS, MAX_TRAP_HITS, MAX_TRAPS,
    MAX_VISIBLE_TRAPS, RockDrop, RockPose, Rockfall, Trap, TrapEffect, TrapId, TrapKind,
    TrapRuntime, TrapTrigger, TrapView, TriggerVolume,
};
pub use vendor::{
    MAX_VENDOR_OFFERS, VENDOR_REACH_UNITS, VendorOffer, VendorStock, VendorStockError,
    VendorTradeError, sell_price, settle_purchase, settle_sale, starter_vendor_stock,
};
pub use water::{AirPocket, Breath, WaterChannel, WaterMode, WaterVolume};
pub use waystone::{
    GREYHAVEN_OUTPOST, GREYHAVEN_ZONE, MAX_WAYSTONES, SEAM_TEST_EAST, SEAM_TEST_WEST,
    WAYSTONE_DISCOVERY_UNITS, WAYSTONES, WaystoneId, WaystoneSet, WaystoneSite, waystone,
    waystone_within_reach,
};
pub use zone::{MAX_PENDING_INTENTS, ZoneSimulation, ZoneTickWork};

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
    /// Tends or mends aid task `task` of the night raid's aftermath (#531, Atlas 5.1.51): an
    /// index into the raid site's authored tasks. The zone checks that it is open and within
    /// reach in the tick.
    RaidAid { task: u8 },
    /// One preparation step before a forewarned night raid (#532, Atlas 5.1.59–5.1.62); the zone
    /// checks the evening, reach and order in the tick.
    RaidPrepare { prep: RaidPrep },
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
