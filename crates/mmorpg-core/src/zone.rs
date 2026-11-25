//! The zone simulation: construction, admission, commands, player movement
//! and the fixed tick order. Combat, creature AI, projections and canonical
//! snapshots are further `impl ZoneSimulation` blocks in their own modules.

use std::collections::BTreeMap;
use std::sync::Arc;

use crate::physics::{RigidBody, Vec3i, World};
use sim_movement::walk;

use crate::creature::CreatureState;
use crate::entity::{WORLD_LIMIT_BODY_BASE, body_id};
use crate::interest::InterestIndex;
use crate::rng::ZoneRng;
use crate::snapshot::PlayerIntent;
use crate::trig::{self, YAW_QUARTER_TURN};
use crate::unit::{PLAYER_START_LEVEL, player_max_health};
use crate::{
    CreatureId, EntityRef, InterestMaintenanceStats, JUMP_VELOCITY_SUBUNITS_PER_TICK,
    MAX_CONTENT_COORDINATE_UNITS, MAX_PLAYERS_PER_ZONE, PLAYER_HALF_EXTENTS_UNITS, PlayerId,
    ZoneCommand, ZoneContent, ZoneDefinition, ZoneError, ZoneEvent, ZoneId,
};

/// Discrete intents (targeting, attacking, releasing) a player may queue
/// between two ticks. Further intents are accepted and dropped, and the next
/// tick reports [`crate::ErrorCode::TooManyIntents`] once.
pub const MAX_PENDING_INTENTS: usize = 16;

/// How far ahead of a player's last accepted command sequence a new one may be (#1151). Clients
/// number their commands consecutively and keep the count across a resume and a handoff, so a
/// gap only spans commands that never arrived: 65 536 is almost an hour of 20 Hz movement
/// resends. A sequence further ahead fails closed, so one garbage value (`u32::MAX`) cannot
/// exhaust the player's sequences for the rest of its session.
pub const MAX_SEQUENCE_GAP: u32 = 1 << 16;

/// Separates the loot streams from the combat streams of the same seed.
pub(crate) const LOOT_STREAM_SALT: u64 = 0x6c6f_6f74_2f76_3031;

/// The engine sweeps motion, so any positive thickness stops a body; a thick
/// slab also keeps contact correction pushing overlapping bodies back inside.
const WORLD_LIMIT_THICKNESS_UNITS: i32 = 1_000;
pub(crate) const PLAYER_HALF_EXTENTS: Vec3i = Vec3i::new(
    PLAYER_HALF_EXTENTS_UNITS[0],
    PLAYER_HALF_EXTENTS_UNITS[1],
    PLAYER_HALF_EXTENTS_UNITS[2],
);

/// One validated movement axis; the wire and canonical form is `-1`, `0` or `1`.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) enum Axis {
    Negative,
    #[default]
    Zero,
    Positive,
}

impl Axis {
    pub(crate) fn new(value: i8) -> Result<Self, ZoneError> {
        match value {
            -1 => Ok(Self::Negative),
            0 => Ok(Self::Zero),
            1 => Ok(Self::Positive),
            _ => Err(ZoneError::new(
                "movement components must be between -1 and 1",
            )),
        }
    }

    pub(crate) const fn get(self) -> i8 {
        match self {
            Self::Negative => -1,
            Self::Zero => 0,
            Self::Positive => 1,
        }
    }
}

/// Authoritative state of one admitted player.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct PlayerState {
    pub(crate) facing: u16,
    pub(crate) forward: Axis,
    pub(crate) strafe: Axis,
    pub(crate) jump_pending: bool,
    /// Held swim ascent (+1) or descent (-1) (#274).
    pub(crate) rise: Axis,
    /// Breath and drowning clock (#274).
    pub(crate) breath: crate::Breath,
    /// The held shield guard and its break recovery (#156).
    pub(crate) guard: crate::GuardState,
    /// Airborne state and landing recovery of every class; Warden melee
    /// waits it out (#155).
    pub(crate) landing: crate::LandingState,
    /// In deep water after the last physics step: the action graph's swim
    /// (#413). Derived from the body's position, so not canonical; a
    /// restored zone derives it again.
    pub(crate) swimming: bool,
    /// A climb or mantle in progress (#490); canonical.
    pub(crate) traversal: crate::Traversal,
    /// The ground or seat the player sits on (#619); canonical.
    pub(crate) sitting: crate::Sitting,
    /// A pending press, a dodge roll in progress or its cooldown (#516); canonical.
    pub(crate) roll: crate::Roll,
    /// The `[forward, up]` launch (cm/s) of an edge's follow-up resolved this
    /// tick (#413). Set by intents and spent by the same tick's movement, so
    /// never present between ticks and not canonical.
    pub(crate) impulse: Option<[i32; 2]>,
    /// A running authored action of the experimental graph (#414). Zone
    /// setup like the graph itself, so not canonical: a restored zone runs
    /// the baseline, which authors none.
    pub(crate) authored: Option<crate::action::authored::AuthoredRun>,
    /// Pressed and released ability inputs, in arrival ticks, while an
    /// experimental graph has hold-to-charge or chord triggers (#414). Zone
    /// setup like a running authored action, so not canonical.
    pub(crate) holds: crate::action::authored::Holds,
    /// A confirmed slash's pending Shield Bash follow-up (#157).
    pub(crate) combo: Option<crate::ComboOpening>,
    /// Ticks left of a directed strike's committed recovery; a guard raised
    /// meanwhile waits to protect (#156, Atlas 5.2.39, 5.2.41).
    pub(crate) strike_recovery: u16,
    /// A running timed swing (#679); canonical.
    pub(crate) swing: Option<crate::Swing>,
    /// The basic strike's chain and pending strike (#885); canonical.
    pub(crate) melee: crate::MeleeState,
    /// The destructible piece the player is repairing (#305).
    pub(crate) repairing: Option<crate::PieceId>,
    pub(crate) last_sequence: u32,
    pub(crate) spawn_slot: u16,
    pub(crate) level: u8,
    pub(crate) experience: u32,
    pub(crate) copper: u32,
    pub(crate) inventory: crate::revisioned::Revisioned<crate::Inventory>,
    pub(crate) inventory_revision: u64,
    /// Equipped items; they share the bag's revision and sheet.
    pub(crate) equipment: crate::revisioned::Revisioned<crate::Equipment>,
    /// Zero means dead.
    pub(crate) health: u32,
    pub(crate) target: Option<EntityRef>,
    /// A pressed swing (#517) waiting for the swing timer; it resolves toward `facing`.
    pub(crate) swing_queued: bool,
    /// Ticks until the next swing may happen.
    pub(crate) swing_timer: u16,
    /// Ticks the player stays in combat after dealing or taking an attack.
    pub(crate) combat_timer: u16,
    /// The owner's explicit draw/sheathe choice; not the threat timer (#802).
    pub(crate) weapon_drawn: bool,
    /// Ticks spent out of combat, driving regeneration.
    pub(crate) calm_ticks: u16,
    /// Discrete intents in sequence order, consumed by the next tick.
    pub(crate) intents: Vec<crate::QueuedIntent>,
    /// A well-formed intent arrived while `intents` was full and was dropped.
    pub(crate) intents_dropped: bool,
    /// The latest zone tick a command reported having observed (#415).
    pub(crate) last_observed: u64,
    /// Events of the current tick.
    pub(crate) events: Vec<ZoneEvent>,
    /// The one-time class choice; `None` keeps the class-agnostic baseline.
    pub(crate) class: Option<crate::ClassChoice>,
    /// The class resource and its regeneration timers (zero without a class).
    pub(crate) resource: crate::class::ResourceClock,
    pub(crate) global_cooldown: u16,
    /// The length the running global cooldown began with, which haste may shorten (#884);
    /// the action bar sweeps over it (#885).
    pub(crate) global_cooldown_total: u16,
    /// Running cooldowns ordered by ability.
    pub(crate) cooldowns: Vec<crate::Cooldown>,
    pub(crate) cast: Option<crate::CastState>,
    /// Auras in slot order.
    pub(crate) auras: Vec<crate::Aura>,
    /// Divine Reckoning pools (#936) in creature order; canonical.
    pub(crate) reckoning: Vec<crate::Harm>,
    /// The first tick the player may speak again.
    pub(crate) chat_ready_at: u64,
    /// Chat lines heard this tick.
    pub(crate) chat: Vec<crate::ChatLine>,
    /// Active and turned-in quests.
    pub(crate) quests: crate::revisioned::Revisioned<crate::QuestLog>,
    /// Waystones the character unlocked by exploration (#145).
    pub(crate) waystones: crate::revisioned::Revisioned<crate::WaystoneSet>,
    /// Named areas the character discovered (#916): each paid its discovery experience once.
    pub(crate) discoveries:
        crate::revisioned::Revisioned<std::collections::BTreeSet<crate::AreaId>>,
    /// Weapon-family mastery (#915), durable, and the practice banked against living foes.
    pub(crate) mastery: crate::WeaponMastery,
    pub(crate) practice: Vec<crate::Practice>,
    /// Discovery caches the character opened (#913), each rewarded once.
    pub(crate) caches: std::collections::BTreeSet<u32>,
    /// The durable character bound to this session (`None` for session-only players), and its
    /// depths receipts (#404).
    pub(crate) character: Option<crate::CharacterId>,
    pub(crate) depths: crate::revisioned::Revisioned<crate::DepthsRecord>,
    /// Physique sliders (#633); their stat effect is derived, never stored.
    pub(crate) physique: crate::Physique,
}

impl PlayerState {
    pub(crate) fn new(spawn_slot: u16) -> Self {
        Self {
            facing: 0,
            forward: Axis::Zero,
            strafe: Axis::Zero,
            jump_pending: false,
            rise: Axis::Zero,
            breath: crate::Breath::FULL,
            guard: crate::GuardState::default(),
            landing: crate::LandingState::default(),
            swimming: false,
            traversal: crate::Traversal::Free,
            sitting: crate::Sitting::No,
            roll: crate::Roll::Ready,
            impulse: None,
            authored: None,
            holds: crate::action::authored::Holds::default(),
            combo: None,
            strike_recovery: 0,
            swing: None,
            repairing: None,
            last_sequence: 0,
            spawn_slot,
            level: PLAYER_START_LEVEL,
            experience: 0,
            copper: 0,
            inventory: crate::Inventory::starter().into(),
            inventory_revision: 1,
            equipment: crate::Equipment::default().into(),
            health: player_max_health(PLAYER_START_LEVEL),
            target: None,
            swing_queued: false,
            swing_timer: 0,
            melee: crate::MeleeState::default(),
            combat_timer: 0,
            weapon_drawn: false,
            calm_ticks: 0,
            intents: Vec::new(),
            intents_dropped: false,
            last_observed: 0,
            events: Vec::new(),
            class: None,
            resource: crate::class::ResourceClock::default(),
            global_cooldown: 0,
            global_cooldown_total: 0,
            cooldowns: Vec::new(),
            cast: None,
            auras: Vec::new(),
            reckoning: Vec::new(),
            chat_ready_at: 0,
            chat: Vec::new(),
            quests: crate::QuestLog::default().into(),
            waystones: crate::WaystoneSet::EMPTY.into(),
            discoveries: std::collections::BTreeSet::new().into(),
            mastery: crate::WeaponMastery::default(),
            practice: Vec::new(),
            caches: std::collections::BTreeSet::new(),
            character: None,
            depths: crate::DepthsRecord::default().into(),
            physique: crate::Physique::NEUTRAL,
        }
    }

    pub(crate) const fn is_alive(&self) -> bool {
        self.health > 0
    }

    /// Level health plus the equipped stamina's bonus, adjusted by physique (#633).
    pub(crate) fn max_health(&self) -> u32 {
        crate::physique::max_health(self.level, &self.equipment, &self.physique)
    }

    /// The class damage bonus of the equipped primary stat.
    pub(crate) fn damage_bonus(&self) -> u16 {
        self.equipment
            .totals()
            .damage_bonus(self.class.map(|choice| choice.class))
    }

    /// The melee range with the equipment bonus on both ends.
    pub(crate) fn melee_damage(&self) -> [u16; 2] {
        let [low, high] = crate::unit::player_damage(self.level);
        let bonus = self.damage_bonus();
        [low + bonus, high + bonus].map(|end| {
            let scaled = self
                .physique
                .apply(crate::PhysiqueAttribute::MeleeDamage, u32::from(end));
            u16::try_from(scaled).unwrap_or(u16::MAX)
        })
    }
}

/// Diagnostic work for the latest attempted tick. Creation and recovery start
/// at zero. These counters never enter canonical state or player projections.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ZoneTickWork {
    /// Living creatures whose AI decision was evaluated, including idle units.
    pub creature_ai_evaluations: usize,
    /// Creature visits of the tick's creature systems (decisions, casts, auras, swings and
    /// corpse timers, #1086): each system visits only the creatures its activity bit holds.
    pub creatures_visited: usize,
    /// Interest query results that outgrew their buffer during the tick, so allocated
    /// (#1086): the creature tick's own queries reuse warm buffers.
    pub interest_query_allocations: usize,
    /// Deferred zone payouts the tick visited, paid or dropped (#1112); at most the tick's
    /// grant bound plus the dropped ones, and none while the backlog is empty.
    pub deferred_rewards_visited: usize,
    /// Expiry of incident receipts (#1128): on a tick whose raid or cart floor rose, one visit
    /// per player with receipts plus one per dropped receipt; otherwise only the previous
    /// tick's dropped grants, and none without reward tracking.
    pub incident_receipts_visited: usize,
    /// Escorts the escort step visited (#1019): each running escort once, following or resting.
    pub escorts_advanced: usize,
    /// Trail points the escort followers examined (#1019): a step's worth per following escort,
    /// however long its trail.
    pub escort_trail_points: usize,
    /// Confrontations checked for a foe out of its fight (#1019): the content's, every tick.
    pub confrontations_checked: usize,
    /// Loads the carry step visited (#1077): each carried or set-down load once.
    pub loads_visited: usize,
    /// Construction sites the construction step visited (#1019): each content site once.
    pub constructions_visited: usize,
    /// Seals and wards the mechanism step visited (#1019): each content seal and ward once.
    pub mechanisms_visited: usize,
    /// Bodies the mechanism step's chamber queries returned (#1019): only while an open seal is due to
    /// close, the bodies in its chamber's box, never the zone's idle players.
    pub seal_bodies_checked: usize,
    /// Raiders on the assault the raid step checked against the objective and, in a defence,
    /// its civilians (#1019): each living assault raider once.
    pub raid_raiders_visited: usize,
    /// Raid sites the raid step visited (#1019): each content site once, every tick.
    pub raid_sites_visited: usize,
    /// Strays the flock step visited (#1019): each content stray once, every tick.
    pub strays_visited: usize,
    /// Ways a driven stray tested for a blocker (#1019): at most five per stray a player drives.
    pub stray_ways_tested: usize,
    /// Prowlers the flock step visited (#1019): each prowler of a running watch once.
    pub prowlers_visited: usize,
    /// Supply patrols the supply step visited (#1019): each content patrol once, every tick.
    pub supply_patrols_visited: usize,
    /// Patrol members the supply step visited (#1019): each member of a run under way or at rest
    /// once.
    pub supply_members_visited: usize,
    /// The engine's own report, present only after a successful physics step.
    pub physics: Option<crate::physics::StepStats>,
}

pub struct ZoneSimulation {
    pub(crate) rewards: Option<crate::RewardHistory>,
    pub(crate) zone_id: ZoneId,
    pub(crate) tick: u64,
    pub(crate) world: World,
    pub(crate) content: Arc<ZoneContent>,
    pub(crate) rng: ZoneRng,
    pub(crate) loot_rng: ZoneRng,
    /// The host's secret world seed (#1111), mixed into every seed the content declares; 0
    /// keeps the content seed alone. Canonical, never projected, not part of the content.
    pub(crate) world_seed: u64,
    pub(crate) players: BTreeMap<PlayerId, PlayerState>,
    pub(crate) creatures: crate::creature_store::CreatureStore,
    pub(crate) interest: InterestIndex,
    pub(crate) interest_work: InterestMaintenanceStats,
    pub(crate) tick_work: ZoneTickWork,
    /// Reused buffers of the creature tick (#1086); never canonical state.
    pub(crate) creature_scratch: crate::creature_store::CreatureScratch,
    /// Door states by ID; absent means closed and never moved (#158).
    pub(crate) doors:
        crate::revisioned::Revisioned<BTreeMap<crate::DoorId, crate::door::DoorState>>,
    /// Flying and stuck projectiles by ID (#286).
    pub(crate) projectiles: BTreeMap<u32, crate::Projectile>,
    pub(crate) next_projectile: u32,
    /// Forced moves in progress and the immunity after them (#287).
    pub(crate) forced: BTreeMap<EntityRef, crate::ForcedState>,
    /// Pieces of broken props by ID (#288).
    pub(crate) debris: BTreeMap<u32, crate::Debris>,
    pub(crate) next_debris: u32,
    /// Destructible pieces' health and last change (#305).
    pub(crate) pieces:
        crate::revisioned::Revisioned<BTreeMap<crate::PieceId, crate::structure::PieceState>>,
    /// Standing summoned terrain by ID (#293), and the next ID.
    pub(crate) summoned_terrain: BTreeMap<u32, crate::SummonedTerrain>,
    pub(crate) next_summoned_terrain: u32,
    /// World traps' firing and hit memory by ID (#300).
    pub(crate) traps: BTreeMap<crate::TrapId, crate::TrapRuntime>,
    /// Navigation cell results (#307), derived from static geometry and dropped when it
    /// changes; not canonical state.
    pub(crate) nav_cache: crate::nav::NavCache,
    /// Standing movable props' exact state (#58).
    pub(crate) prop_motion: BTreeMap<CreatureId, crate::movable::PropMotion>,
    /// The validated action graph the zone runs (#380): the baseline unless
    /// a test or the lab configured another; zone setup, not canonical state.
    pub(crate) action_graph: &'static crate::action::ActionGraph,
    /// The lateness of the intent `consume_intents` is resolving (#415); 0
    /// outside it, so never present between ticks and not canonical.
    pub(crate) resolving_late: u8,
    /// Summoned minions by ID (#295), and the next minion ID.
    pub(crate) minions: crate::revisioned::Revisioned<BTreeMap<crate::MinionId, crate::Minion>>,
    /// The threatened-player set shared by recipient projections (#1006); derived, never canonical.
    pub(crate) threat_cache: crate::threat_cache::ThreatCache,
    /// The zone-wide projection views of the current tick (#1056); derived, never canonical.
    pub(crate) zone_view_cache: crate::zone_views::ZoneViewCache,
    pub(crate) next_minion: u32,
    /// Frozen, staged and activated character transfers (#145); host-phase
    /// state the handoff record reconciles, not canonical.
    pub(crate) transfers: crate::transfer::Transfers,
    /// The party directory's view of each player that has one (#404).
    pub(crate) parties: crate::party::Parties,
    /// The night raid's incidents, objective integrity and last resolution (#530).
    pub(crate) raid: crate::RaidState,
    /// Each further raid site's state (#1019), in content order after the night raid target.
    pub(crate) site_raids: Vec<crate::RaidState>,
    /// Private-home residents a knock woke, by NPC (#158).
    pub(crate) wakes: BTreeMap<crate::NpcId, crate::NpcWake>,
    /// Running quest escorts (#1019), canonical.
    pub(crate) escorts: BTreeMap<crate::NpcId, crate::EscortState>,
    /// Strays away from their scatter points (#1019), canonical.
    pub(crate) strays: BTreeMap<crate::NpcId, crate::StrayState>,
    /// The running or settling watch over the flock (#1019), canonical.
    pub(crate) watch: Option<crate::WatchState>,
    /// Supply patrols' runs under way or at rest (#1019), by lookout, canonical.
    pub(crate) supply_runs: BTreeMap<crate::NpcId, crate::SupplyRun>,
    /// Every player's one load, carried or set down (#1077), canonical.
    pub(crate) loads: BTreeMap<PlayerId, crate::LoadState>,
    /// Every construction site's material and raising (#1019), canonical.
    pub(crate) constructions: BTreeMap<crate::PieceId, crate::ConstructionState>,
    /// Every seal's set mechanisms and opening, and every ward's exposure (#1019), canonical, in
    /// content order.
    pub(crate) seals: Vec<crate::SealState>,
    pub(crate) wards: Vec<crate::WardState>,
    /// The private run bound to this depths zone (#404); `None` in every other zone.
    pub(crate) depths: Option<crate::DepthsRun>,
    /// Where bodies stood over the last ticks, for capped hit rewind (#680); canonical,
    /// empty without it.
    pub(crate) rewind_history: crate::rewind::RewindHistory,
    /// When each ore vein is full again and which corpses were skinned (#917); canonical.
    pub(crate) gathering: crate::revisioned::Revisioned<crate::GatheringState>,
}

impl ZoneSimulation {
    /// An empty, zero-gravity test world without units.
    #[must_use]
    pub fn new(zone_id: ZoneId) -> Self {
        Self::with_definition(zone_id, ZoneDefinition::default())
            .expect("empty zone definition has no invalid physics bodies")
    }

    /// Physical content only; see [`ZoneContent::from_definition`].
    pub fn with_definition(zone_id: ZoneId, definition: ZoneDefinition) -> Result<Self, ZoneError> {
        Self::with_content(zone_id, Arc::new(ZoneContent::from_definition(definition)))
    }

    /// A fresh zone at tick 0: colliders, NPCs and every creature at its
    /// spawn point. Creature levels and first wander delays are drawn from
    /// the zone RNG in creature-ID order.
    pub fn with_content(zone_id: ZoneId, content: Arc<ZoneContent>) -> Result<Self, ZoneError> {
        Self::with_world_seed(zone_id, content, 0)
    }

    /// [`Self::with_content`] for a world whose host drew `world_seed` (#1111): every random
    /// draw derives from the content seed mixed with it, so the public content alone does not
    /// tell what a creature drops or which night the raid comes. 0 is the content seed alone.
    /// The seed is fixed at creation and kept by canonical snapshots.
    pub fn with_world_seed(
        zone_id: ZoneId,
        content: Arc<ZoneContent>,
        world_seed: u64,
    ) -> Result<Self, ZoneError> {
        let mut zone = Self::without_creatures(zone_id, content, world_seed)?;
        let spawns: Vec<_> = zone
            .content
            .creature_spawns()
            .iter()
            .map(|spawn| spawn.id)
            .collect();
        for id in spawns {
            // Night raiders (#530) stand only once an incident commits them, and the hunt's beast
            // (#991) only on a confrontation day, and the cart's bandits (#992) on an ambush.
            if zone.content.is_raider(id)
                || zone.content.is_depths_creature(id)
                || zone.content.is_hunt_beast(id)
                || zone.content.is_cart_bandit(id)
                || zone.content.is_confrontation_foe(id)
                || zone.content.is_prowler(id)
                || zone.content.supply_patrol_of(id).is_some()
            {
                zone.reserve_raider(id)?;
            } else {
                zone.spawn_creature(id)?;
            }
        }
        Ok(zone)
    }

    /// Static world, NPC bodies and a freshly seeded RNG; recovery adds the
    /// recorded units on top.
    pub(crate) fn without_creatures(
        zone_id: ZoneId,
        content: Arc<ZoneContent>,
        world_seed: u64,
    ) -> Result<Self, ZoneError> {
        let definition = content.definition();
        let gravity = definition.gravity();
        let mut world = World::new(Vec3i::new(gravity[0], gravity[1], gravity[2]));
        let spawn = definition.spawn_grid().feet(0).unwrap_or([0; 3]);
        for collider in definition.colliders() {
            if let Some(mesh) = definition
                .static_meshes()
                .iter()
                .find(|mesh| mesh.id == collider.id)
            {
                world
                    .add_body(RigidBody::static_mesh(mesh).map_err(physics_error)?)
                    .map_err(physics_error)?;
                continue;
            }
            // The flat ground the spawn grid stands on is a floor half-space at y = 0 (#271).
            if is_ground(collider, spawn) {
                let id = crate::physics::BodyId(u64::from(collider.id));
                world
                    .add_body(RigidBody::floor(id, 0))
                    .map_err(physics_error)?;
                // Authored hills rise from the flat ground under the same identity (#159).
                if let Some(terrain) = definition.terrain() {
                    world.set_terrain(id, terrain.clone());
                }
                continue;
            }
            let id = crate::physics::BodyId(u64::from(collider.id));
            let (position, half) = (vector(collider.position), vector(collider.half_extents));
            // A rock (#49) collides as its authored convex proxy inside its box.
            let body = match content.rock(collider.id) {
                Some(shape) => RigidBody::rock(id, position, half, shape),
                None => RigidBody::fixed(id, position, half),
            };
            world.add_body(body).map_err(physics_error)?;
        }
        for body in world_limit_bodies() {
            world.add_body(body).map_err(physics_error)?;
        }
        // Every door starts closed.
        for door in content.doors() {
            world
                .add_body(RigidBody::fixed(
                    crate::physics::BodyId(u64::from(door.body)),
                    vector(door.closed.position),
                    vector(door.closed.half_extents),
                ))
                .map_err(physics_error)?;
        }
        // Ramps (#59) are fixed wedges.
        for ramp in content.ramps() {
            world
                .add_body(RigidBody::ramp(
                    crate::physics::BodyId(u64::from(ramp.body)),
                    vector(ramp.position),
                    vector(ramp.half_extents),
                    ramp.rises,
                ))
                .map_err(physics_error)?;
        }
        // Destructible pieces (#305) stand intact.
        let pieces = Self::build_pieces(&mut world, &content)?;
        let constructions = Self::build_constructions(&content);
        let (seals, wards) = Self::build_mechanisms(&content);
        // Platforms (#56) start at their tick-0 centre.
        for platform in content.platforms() {
            let [x, y, z] = platform.centre_at(0);
            world
                .add_body(RigidBody::platform(
                    crate::physics::BodyId(u64::from(platform.body)),
                    Vec3i::new(x, y, z),
                    vector(platform.half_extents),
                ))
                .map_err(physics_error)?;
        }
        // Each further raid site keeps its own state (#1019).
        let site_raids = vec![crate::RaidState::default(); content.raids().len().saturating_sub(1)];
        let mut interest = InterestIndex::default();
        for npc in content.npcs() {
            let entity = EntityRef::Npc(npc.id);
            world
                .add_body(RigidBody::fixed(
                    body_id(entity),
                    Vec3i::new(
                        npc.position[0],
                        definition.ground_at(npc.position[0], npc.position[1])
                            + PLAYER_HALF_EXTENTS_UNITS[1],
                        npc.position[1],
                    ),
                    PLAYER_HALF_EXTENTS,
                ))
                .map_err(physics_error)?;
            interest.insert(entity, npc.position[0], npc.position[1]);
        }
        // The debris, prop and boulder response worlds hold the static world from the start, so
        // the first break does not build them (#666); later static changes sync incrementally.
        world.prepare_response_lanes().map_err(physics_error)?;
        Ok(Self {
            zone_id,
            tick: 0,
            world,
            rng: ZoneRng::seeded(content.rng_seed() ^ world_seed, zone_id),
            loot_rng: ZoneRng::seeded(content.rng_seed() ^ world_seed ^ LOOT_STREAM_SALT, zone_id),
            world_seed,
            traps: Self::fresh_traps(&content),
            gathering: crate::GatheringState::full(content.gather_nodes()).into(),
            content,
            players: BTreeMap::new(),
            creatures: crate::creature_store::CreatureStore::default(),
            interest,
            interest_work: InterestMaintenanceStats::default(),
            tick_work: ZoneTickWork::default(),
            creature_scratch: crate::creature_store::CreatureScratch::default(),
            doors: crate::revisioned::Revisioned::default(),
            projectiles: BTreeMap::new(),
            next_projectile: 1,
            forced: BTreeMap::new(),
            debris: BTreeMap::new(),
            next_debris: 1,
            summoned_terrain: BTreeMap::new(),
            next_summoned_terrain: 1,
            pieces: pieces.into(),
            nav_cache: BTreeMap::new(),
            prop_motion: BTreeMap::new(),
            action_graph: &crate::action::BASELINE,
            resolving_late: 0,
            minions: crate::revisioned::Revisioned::default(),
            threat_cache: crate::threat_cache::ThreatCache::default(),
            zone_view_cache: crate::zone_views::ZoneViewCache::default(),
            next_minion: 1,
            raid: crate::RaidState::default(),
            site_raids,
            wakes: BTreeMap::new(),
            escorts: BTreeMap::new(),
            strays: BTreeMap::new(),
            watch: None,
            supply_runs: BTreeMap::new(),
            loads: BTreeMap::new(),
            constructions,
            seals,
            wards,
            depths: None,
            rewind_history: crate::rewind::RewindHistory::new(),
            rewards: None,
            transfers: crate::transfer::Transfers::default(),
            parties: crate::party::Parties::new(),
        })
    }

    /// The immutable content this zone simulates.
    #[must_use]
    pub const fn content(&self) -> &Arc<ZoneContent> {
        &self.content
    }

    /// The collision authority selected for this immutable zone content, including recovery.
    #[must_use]
    pub fn physics_backend(&self) -> crate::physics::Backend {
        self.world.backend()
    }

    /// The seed every runtime draw derives from (#1111): the content's declared seed mixed
    /// with the world seed. Never projected.
    pub(crate) fn rng_seed(&self) -> u64 {
        self.content.rng_seed() ^ self.world_seed
    }

    #[must_use]
    pub fn definition(&self) -> &ZoneDefinition {
        self.content.definition()
    }

    #[must_use]
    pub const fn zone_id(&self) -> ZoneId {
        self.zone_id
    }

    #[must_use]
    pub const fn current_tick(&self) -> u64 {
        self.tick
    }

    #[must_use]
    pub fn player_count(&self) -> usize {
        self.players.len()
    }

    /// Cumulative work since creation or recovery; subtract readings around a workload.
    #[must_use]
    pub const fn interest_maintenance_stats(&self) -> InterestMaintenanceStats {
        self.interest_work
    }

    #[must_use]
    pub const fn tick_work(&self) -> ZoneTickWork {
        self.tick_work
    }

    /// With `on`, every creature system visits every creature each tick, as before #1086:
    /// the simple reference that tests compare the activity-driven tick against. Never
    /// changes an outcome, only the work.
    #[doc(hidden)]
    pub fn visit_every_creature(&mut self, on: bool) {
        self.creatures.set_exhaustive(on);
    }

    /// Interest query results of this zone's indexes that outgrew their buffer.
    fn interest_query_allocations(&self) -> usize {
        self.interest.allocations() + self.creatures.resting_allocations()
    }

    /// How many creatures the decision step would visit now (#1086); workload evidence only.
    #[doc(hidden)]
    #[must_use]
    pub fn awake_creatures(&self) -> usize {
        self.creatures
            .active_count(crate::creature_store::Activity::Decide)
    }

    pub fn add_player(&mut self, player_id: PlayerId) -> Result<(), ZoneError> {
        if self.players.contains_key(&player_id) {
            return Err(ZoneError::new("player already exists in zone"));
        }
        if self.players.len() >= MAX_PLAYERS_PER_ZONE {
            return Err(ZoneError::new("zone player capacity reached"));
        }

        let spawn_slot = self
            .available_spawn_slot()
            .ok_or_else(|| ZoneError::new("zone spawn capacity reached"))?;
        let position = self.spawn_position(spawn_slot)?;
        let entity = EntityRef::Player(player_id);
        self.world
            .add_body(RigidBody::dynamic(
                body_id(entity),
                position,
                Vec3i::ZERO,
                PLAYER_HALF_EXTENTS,
            ))
            .map_err(physics_error)?;

        self.interest.insert(entity, position.x, position.z);
        self.interest_work.bucket_inserts += 1;
        self.players.insert(player_id, PlayerState::new(spawn_slot));
        Ok(())
    }

    /// Removes the player's unit at once: its body, its place on every
    /// threat table and every other player's selection of it.
    pub fn remove_player(&mut self, player_id: PlayerId) -> bool {
        // A carried load is set down where its carrier stood, footing allowing (#1077).
        if self.players.contains_key(&player_id) && self.release_load(player_id).is_err() {
            self.loads.remove(&player_id);
        }
        if self.players.remove(&player_id).is_none() {
            return false;
        }
        self.parties.remove(&player_id);
        self.transfers.waystone_departures.remove(&player_id);
        if let Some(rewards) = &mut self.rewards {
            rewards.remove_player(player_id);
        }
        let entity = EntityRef::Player(player_id);
        self.world.remove_body(body_id(entity));
        if self.interest.remove(entity) {
            self.interest_work.bucket_removes += 1;
        }
        self.forget_caster(entity);
        self.forget_forced_unit(entity);
        // Its minions vanish with it (#295).
        for minion in self.minions_of(player_id) {
            self.dismiss_minion(minion);
        }
        // Only the creatures it concerns change (#1086).
        self.creatures.update_where(
            |creature| {
                creature.threat.iter().any(|entry| entry.entity == entity)
                    || creature
                        .cast
                        .is_some_and(|cast| cast.target == Some(entity))
                    || creature.tapped_by == Some(player_id)
                    || creature.loot_party.contains(&player_id)
            },
            |creature| {
                crate::creature::forget_departed(&mut creature.threat, &mut creature.cast, entity);
                crate::creature::release_departed_loot(
                    &mut creature.tapped_by,
                    &mut creature.loot_party,
                    &mut creature.loot,
                    player_id,
                );
            },
        );
        for player in self.players.values_mut() {
            if player.cast.is_some_and(|cast| cast.target == Some(entity)) {
                player.cast = None;
            }
            if player.combo.is_some_and(|combo| combo.target == entity) {
                player.combo = None;
            }

            if player.target == Some(entity) {
                player.target = None;
            }
        }
        true
    }

    /// Validates a command and records it for the next tick. Movement is
    /// held intent; discrete intents queue in sequence order and resolve in
    /// the tick. A malformed or stale command changes nothing. A well-formed
    /// intent beyond [`MAX_PENDING_INTENTS`] is a gameplay outcome, not an
    /// error: it consumes its sequence, is dropped, and the next tick reports
    /// it with an `Error` event. Dead units cannot move: while dead, `Move`
    /// and `Jump` consume their sequence and hold no intent, so nothing held
    /// before or during death moves the released spirit.
    pub fn apply_command(
        &mut self,
        player_id: PlayerId,
        sequence: u32,
        command: ZoneCommand,
    ) -> Result<(), ZoneError> {
        self.apply_command_observed(player_id, sequence, command, None)
    }

    /// [`Self::apply_command`] for a command sent after the client had
    /// observed the zone's completed tick `observed` (Atlas 5.9.6, #415). Its
    /// discrete intent is late by the ticks completed since, at most
    /// [`crate::action::LATE_INPUT_GRACE_TICKS`], which an action-graph edge
    /// window may make up for. An observation ahead of the zone, or older
    /// than the player's previous one, fails closed like a stale sequence.
    pub fn apply_observed_command(
        &mut self,
        player_id: PlayerId,
        sequence: u32,
        command: ZoneCommand,
        observed: u64,
    ) -> Result<(), ZoneError> {
        self.apply_command_observed(player_id, sequence, command, Some(observed))
    }

    fn apply_command_observed(
        &mut self,
        player_id: PlayerId,
        sequence: u32,
        command: ZoneCommand,
        observed: Option<u64>,
    ) -> Result<(), ZoneError> {
        let now = self.tick;
        let tracks_holds = self.action_graph.tracks_holds();
        let player = self
            .players
            .get_mut(&player_id)
            .ok_or_else(|| ZoneError::new("unknown player"))?;
        if sequence == 0 || sequence <= player.last_sequence {
            return Err(ZoneError::new("command sequence is stale"));
        }
        if sequence - player.last_sequence > MAX_SEQUENCE_GAP {
            return Err(ZoneError::new("command sequence is too far ahead"));
        }
        if let Some(observed) = observed {
            if observed > now {
                return Err(ZoneError::new("command observation is ahead of the zone"));
            }
            if observed < player.last_observed {
                return Err(ZoneError::new("command observation is stale"));
            }
        }
        let late = observed.map_or(0, |observed| {
            u8::try_from(now - observed)
                .unwrap_or(u8::MAX)
                .min(crate::action::LATE_INPUT_GRACE_TICKS)
        });

        // Validate completely before mutating: a malformed command changes nothing.
        let alive = player.is_alive();
        let intent = match command {
            ZoneCommand::Move {
                forward,
                strafe,
                facing,
            } => {
                let forward = Axis::new(forward)?;
                let strafe = Axis::new(strafe)?;
                if alive {
                    player.forward = forward;
                    player.strafe = strafe;
                    player.facing = facing;
                }
                None
            }
            ZoneCommand::Rise(rise) => {
                let rise = Axis::new(rise)?;
                if alive {
                    player.rise = rise;
                }
                None
            }
            // Resolved during the next tick against the pre-step world.
            ZoneCommand::Jump => {
                player.jump_pending = alive;
                None
            }
            // Resolved like a jump (#516); a press while rolling or recovering is dropped.
            ZoneCommand::Roll => {
                if alive && player.roll == crate::Roll::Ready {
                    player.roll = crate::Roll::Pending;
                }
                None
            }
            // Queued so a guard pressed after a strike stays raised (5.2.37).
            ZoneCommand::Guard { lane } => Some(PlayerIntent::Guard(lane)),
            ZoneCommand::SetWeaponDrawn(drawn) => Some(PlayerIntent::SetWeaponDrawn(drawn)),
            ZoneCommand::UseDoor { door, open } => Some(PlayerIntent::UseDoor { door, open }),
            ZoneCommand::Sit { seat } => Some(PlayerIntent::Sit(seat)),
            ZoneCommand::Repair { piece } => Some(PlayerIntent::Repair(piece)),
            ZoneCommand::SelectTarget(target) => Some(PlayerIntent::SelectTarget(target)),
            ZoneCommand::Swing { facing } => Some(PlayerIntent::Swing { facing }),
            ZoneCommand::Loot(claim) => Some(PlayerIntent::Loot(claim)),
            ZoneCommand::ReleaseSpirit => Some(PlayerIntent::ReleaseSpirit),
            ZoneCommand::MoveItem {
                source,
                destination,
                quantity,
            } => Some(PlayerIntent::MoveItem {
                source,
                destination,
                quantity,
            }),
            ZoneCommand::UseAbility {
                ability,
                target,
                facing,
            } => Some(PlayerIntent::UseAbility {
                ability,
                target,
                facing,
            }),
            // Held state like movement, in arrival ticks (#414).
            ZoneCommand::ReleaseAbility { ability, retired } => {
                if tracks_holds {
                    player
                        .holds
                        .release(crate::AbilityId::new(ability), now, retired);
                }
                None
            }
            ZoneCommand::DirectedStrike {
                ability,
                facing,
                aim,
            } => Some(PlayerIntent::DirectedStrike {
                ability,
                facing,
                aim,
            }),
            ZoneCommand::CancelCast => Some(PlayerIntent::CancelCast),
            ZoneCommand::CommandMinions(order) => Some(PlayerIntent::CommandMinions(order)),
            ZoneCommand::ChooseClass { class, sex } => {
                Some(PlayerIntent::ChooseClass { class, sex })
            }
            ZoneCommand::EquipItem { bag_slot } => Some(PlayerIntent::EquipItem { bag_slot }),
            ZoneCommand::UnequipItem { equipment_slot } => {
                Some(PlayerIntent::UnequipItem { equipment_slot })
            }
            ZoneCommand::BuyItem {
                npc,
                offer,
                quantity,
            } => Some(PlayerIntent::BuyItem {
                npc,
                offer,
                quantity,
            }),
            ZoneCommand::SellItem {
                npc,
                bag_slot,
                quantity,
            } => Some(PlayerIntent::SellItem {
                npc,
                bag_slot,
                quantity,
            }),
            ZoneCommand::Chat { channel, text } => Some(PlayerIntent::Chat(channel.message(text))),
            ZoneCommand::Emote(emote) => Some(PlayerIntent::Chat(crate::ChatMessage::Emote(emote))),
            ZoneCommand::AcceptQuest { npc, quest } => {
                Some(PlayerIntent::AcceptQuest { npc, quest })
            }
            ZoneCommand::CompleteQuest { npc, quest, choice } => {
                Some(PlayerIntent::CompleteQuest { npc, quest, choice })
            }
            ZoneCommand::AbandonQuest { quest } => Some(PlayerIntent::AbandonQuest { quest }),
            ZoneCommand::UseWaystone { waystone } => Some(PlayerIntent::UseWaystone(waystone)),
            ZoneCommand::RaidAid { task } => Some(PlayerIntent::RaidAid(task)),
            ZoneCommand::RaidPrepare { prep } => Some(PlayerIntent::RaidPrepare(prep)),
            ZoneCommand::Gather { node } => Some(PlayerIntent::Gather(node)),
            ZoneCommand::Inspect { spot } => Some(PlayerIntent::Inspect(spot)),
            ZoneCommand::QuestAction { npc } => Some(PlayerIntent::QuestAction(npc)),
            ZoneCommand::Decide { npc, option } => Some(PlayerIntent::Decide(npc, option)),
            ZoneCommand::Operate { mechanism } => Some(PlayerIntent::Operate(mechanism)),
            ZoneCommand::Lift { source } => Some(PlayerIntent::Lift(source)),
            ZoneCommand::SetDown => Some(PlayerIntent::SetDown),
            ZoneCommand::Skin { creature } => Some(PlayerIntent::Skin(creature)),
            ZoneCommand::RollDepthsDrop { run, drop, choice } => {
                Some(PlayerIntent::RollDepthsDrop { run, drop, choice })
            }
            ZoneCommand::ClaimDepthsDrop { run, drop } => {
                Some(PlayerIntent::ClaimDepthsDrop { run, drop })
            }
        };
        if let Some(intent) = intent {
            if player.intents.len() < MAX_PENDING_INTENTS {
                // A press an experimental hold-to-charge or chord may read
                // (#414); the production graph tracks none.
                if tracks_holds && let PlayerIntent::UseAbility { ability, .. } = intent {
                    player.holds.press(crate::AbilityId::new(ability), now);
                }
                player.intents.push(crate::QueuedIntent { intent, late });
            } else {
                player.intents_dropped = true;
            }
        }
        if let Some(observed) = observed {
            player.last_observed = observed;
        }
        player.last_sequence = sequence;
        Ok(())
    }

    /// Advances one tick in a fixed order:
    ///
    /// 0. with reward tracking, zone payouts deferred by earlier ticks, oldest first, while
    ///    the tick has room (#1112);
    /// 1. every player's pending intents, in `(player id, sequence)` order,
    ///    including class choices and ability use (instant abilities resolve
    ///    here, casts and channels start here);
    /// 2. creature AI decisions in creature-ID order (aggro, assist, chase,
    ///    wander, leash, evade and creature ability casts), which set
    ///    creature velocities; roots and stuns stop them, snares slow them; then minion
    ///    decisions in minion-ID order (#295: orders, fights, formation and separation);
    /// 3. player movement velocities and grounded jumps (dead and stunned
    ///    players stand still); movement or a jump interrupts a cast; large
    ///    creatures shove smaller units in their way (#297) and forced moves
    ///    set their units' velocities;
    /// 4. moving platforms take their scheduled centre for this tick (#56), carrying their
    ///    riders in one physics step, then one debris step for broken props' pieces (#288);
    /// 5. interest index maintenance from the new positions;
    /// 6. combat: global cooldowns, cooldowns and casts or channels (players,
    ///    then creatures), aura pulses and expiry in `(EntityRef, aura slot)`
    ///    order, then swings in `EntityRef` order: swing timers, range checks
    ///    after movement, damage, threat, deaths, tapping (minions after creatures), then
    ///    world traps (#300) in ID order, which may kill;
    /// 7. minions whose time ran out crumble, then health and class-resource regeneration,
    ///    corpse despawns and respawns in ID order, structure regrowth, then the night raid
    ///    (#530: barn hits, phases, outcome, marches and the nightly trigger), the hunt's beast
    ///    standing or withdrawing (#991), the cart's bandits standing, rewarding defenders or
    ///    withdrawing (#992), and a depths run's reinforcements, cleared chambers and wipe
    ///    (#404);
    /// 8. talk and explore quest objectives of living players in player-ID
    ///    order;
    /// 9. the tick counter, then, with reward tracking, the receipts of raid and cart incidents
    ///    that can no longer pay expire when this tick raised the zone's incident floors
    ///    (#1128; the previous tick's grants clear first, and their expired receipts with them).
    ///
    /// Events of the previous tick are cleared first, so each player's queue
    /// holds exactly this tick's events until the next tick starts.
    pub fn advance_tick(&mut self) -> Result<(), ZoneError> {
        let now = self
            .tick
            .checked_add(1)
            .ok_or_else(|| ZoneError::new("zone tick overflow"))?;
        self.tick_work = ZoneTickWork::default();
        // The previous tick's grants clear; the floors are this tick's starting point (#1128).
        let floors = self.begin_reward_tick();
        let queries = self.interest_query_allocations();
        for player in self.players.values_mut() {
            player.events.clear();
            player.chat.clear();
        }
        // Zone payouts that did not fit an earlier tick come first (#1112).
        self.pay_deferred_rewards()?;
        self.consume_intents()?;
        self.decide_creatures()?;
        self.decide_minions()?;
        self.drive_players()?;
        self.shove_with_large_creatures()?;
        self.apply_forced_moves()?;
        self.schedule_platforms(now, false)?;
        // Read before the step resolves units against the boulders they walk into (#299).
        let boulder_pushes = self.boulder_walk_pushes()?;
        self.tick_work.physics = Some(self.world.step().map_err(physics_error)?);
        self.settle_forced_moves(now)?;
        self.advance_debris();
        self.advance_movable_props()?;
        self.advance_boulders(&boulder_pushes, now)?;
        self.update_landings()?;
        self.update_breath()?;
        // A jump intent is consumed by the tick that evaluated it, grounded or not.
        for player in self.players.values_mut() {
            player.jump_pending = false;
        }
        self.update_interest()?;
        self.discover_waystones();
        self.discover_areas()?;
        self.open_caches()?;
        self.advance_casts(now)?;
        // Timed swings clash, parry and sweep before projectiles fly (#679).
        self.advance_swings(now)?;
        self.clash_projectiles();
        self.advance_projectiles(now)?;
        self.advance_auras(now)?;
        self.resolve_combat(now)?;
        self.advance_traps(now)?;
        self.advance_sitting();
        self.update_timers(now)?;
        self.regrow_pieces(now)?;
        // Construction sites rise, stand their time or come down (#1019).
        self.advance_constructions(now)?;
        // Shrine glyphs drop back and open seals close again (#1019).
        self.advance_mechanisms(now)?;
        self.advance_summoned(now);
        self.advance_raid(now)?;
        self.advance_hunt(now)?;
        self.advance_cart(now)?;
        // Escorts follow, arrive or fail, and confronted foes out of their fight leave (#1019).
        self.advance_npc_quests(now)?;
        // Strays shy, are penned or bolt, and a watch over the flock runs (#1019).
        self.advance_flock(now)?;
        // Supply patrols are sighted, stopped, arrive or rest (#1019).
        self.advance_supply(now)?;
        // Set-down loads run out, and fallen or drowned carriers lose theirs (#1077).
        self.advance_loads(now)?;
        self.advance_depths(now)?;
        self.advance_combos();
        self.advance_authored();
        self.advance_quest_objectives()?;
        // Practice against a foe that died this tick becomes mastery (#915).
        self.settle_practice()?;
        self.tick = now;
        // Receipts of incidents this tick ended expire (#1128).
        self.expire_incident_receipts(floors);
        // Routine NPCs stand where the new tick's time of day puts them (#158).
        self.place_npcs(true)?;
        self.advance_private_homes()?;
        // Where bodies ended this tick, for a match with hit rewind (#680).
        self.record_rewind_frame(now)?;
        self.tick_work.interest_query_allocations =
            self.interest_query_allocations().saturating_sub(queries);
        // A physics query the engine refused this tick, or since the last one, fails the tick
        // instead of panicking the host (#1151).
        self.world.take_query_failure().map_err(physics_error)?;
        Ok(())
    }

    /// Step 3: horizontal controller velocity from intent, vertical velocity
    /// from physics or a grounded jump. Stunned players stand still, roots
    /// and snares scale the controller velocity, and a non-zero movement
    /// intent or a jump interrupts a cast or channel (the action graph's
    /// `MovementCancelsCast`).
    fn drive_players(&mut self) -> Result<(), ZoneError> {
        for player_id in self.moves_cancelling_casts() {
            self.interrupt_cast(EntityRef::Player(player_id), None, None);
        }
        // Moving or jumping stops a repair (#305).
        for player in self.players.values_mut() {
            if player.forward != Axis::Zero || player.strafe != Axis::Zero || player.jump_pending {
                player.repairing = None;
                // So does a sit (#619).
                player.sitting = crate::Sitting::No;
            }
        }
        let probe_ground = self.ground_body();
        let mut traversals = Vec::with_capacity(self.players.len());
        let mut rolls = Vec::with_capacity(self.players.len());
        // Carriers who pressed a dodge roll or jumped at a face (#1077).
        let mut laden = Vec::new();
        for (&player_id, state) in &self.players {
            let body_id = body_id(EntityRef::Player(player_id));
            let body = self
                .world
                .body(body_id)
                .ok_or_else(|| ZoneError::new("player physics body is missing"))?;
            let probe = crate::climb::Probe {
                world: &self.world,
                surfaces: self.content.climbing(),
                ground: probe_ground,
                body: body_id,
            };
            // Setting velocities never moves bodies, so every grounded probe in
            // this pass observes the same pre-step positions regardless of order.
            let alive = state.is_alive();
            let stunned = crate::aura::has_kind(&state.auras, crate::AuraKind::Stun);
            // A living swimmer (#274) has no gravity: held rise intent moves it up or down,
            // and the surface holds it with its head out of the water.
            let centre = body.position();
            let swim_depth = alive
                .then(|| {
                    crate::water::swim_depth(
                        self.content.waters(),
                        [centre.x, centre.y - PLAYER_HALF_EXTENTS_UNITS[1], centre.z],
                    )
                })
                .flatten();
            let grounded = is_grounded(&self.world, body)?;
            // An authored surface hazard under a standing player's feet (#294).
            let surface = (alive && grounded)
                .then(|| {
                    crate::surface::under_feet(
                        self.content.surfaces(),
                        [centre.x, centre.y - PLAYER_HALF_EXTENTS_UNITS[1], centre.z],
                    )
                })
                .flatten();
            // A dodge roll (#516) starts on the ground, out of deep water, off any face and
            // unstunned; a stun or deep water ends it early, and death outright.
            let roll = if alive {
                let mut roll = state.roll.advanced();
                if stunned || swim_depth.is_some() {
                    roll = roll.ended();
                }
                // Both arms round a load, nobody rolls (#1077).
                let carrying = self
                    .loads
                    .get(&player_id)
                    .is_some_and(crate::LoadState::carried);
                if carrying && roll == crate::Roll::Pending {
                    laden.push(player_id);
                }
                let can = !stunned
                    && swim_depth.is_none()
                    && grounded
                    && !state.traversal.holds()
                    && !carrying;
                let yaw = movement_yaw(state)
                    .unwrap_or_else(|| state.facing.wrapping_add(2 * YAW_QUARTER_TURN));
                roll.start(can, yaw)
            } else {
                crate::Roll::Ready
            };
            rolls.push((player_id, roll));
            let rolling = roll.rolling();
            // A climb or mantle (#490) moves the body weightless along the zone's velocity. A
            // roller's intent is set aside until the roll ends, so it grabs nothing.
            let step = if alive && swim_depth.is_none() {
                let held = rolling.is_none();
                crate::climb::decide(
                    &probe,
                    body.exact_position(),
                    state.traversal,
                    crate::climb::Intent {
                        forward: if held {
                            i32::from(state.forward.get())
                        } else {
                            0
                        },
                        strafe: if held {
                            i32::from(state.strafe.get())
                        } else {
                            0
                        },
                        jump: held && state.jump_pending,
                        facing: state.facing,
                        grounded,
                        stunned,
                    },
                )
            } else {
                crate::climb::Step::FREE
            };
            // A carrier grabs no face nor ledge (#1077); jumping at one is refused.
            let carrying = self
                .loads
                .get(&player_id)
                .is_some_and(crate::LoadState::carried);
            let step = if carrying && step.traversal.holds() && !state.traversal.holds() {
                if state.jump_pending && grounded {
                    laden.push(player_id);
                }
                crate::climb::Step::FREE
            } else {
                step
            };
            traversals.push((player_id, step.traversal));
            // The roll moves the body through the controller at its own speed, under gravity.
            if let Some(yaw) = rolling
                && step.velocity.is_none()
            {
                let percent = crate::aura::speed_percent(&state.auras)
                    * crate::surface::speed_percent(surface)
                    / 100;
                let [x, z] = crate::Roll::velocity(yaw, percent);
                self.world
                    .set_velocity(body_id, Vec3i::new(x, body.velocity().y, z))
                    .map_err(physics_error)?;
                self.world
                    .set_weightless(body_id, false)
                    .map_err(physics_error)?;
                continue;
            }
            if let Some(velocity) = step.velocity {
                self.world
                    .set_velocity(body_id, velocity)
                    .map_err(physics_error)?;
                self.world
                    .set_weightless(body_id, true)
                    .map_err(physics_error)?;
                continue;
            }
            let vertical_velocity = if let Some(depth) = swim_depth {
                let rise = if stunned { 0 } else { state.rise.get() };
                crate::water::swim_vertical_velocity(depth, rise)
            } else if let Some(jump) = walk::jump_velocity(
                grounded,
                alive && !stunned && state.jump_pending,
                JUMP_VELOCITY_SUBUNITS_PER_TICK,
            ) {
                jump
            } else if state.traversal.holds() {
                // Letting go of a face or landing a mantle keeps no upward climb.
                body.velocity().y.min(0)
            } else {
                body.velocity().y
            };
            // A protecting guard slows movement (#156): ready and grounded.
            let guarding =
                crate::guard::guard_ready(self.action_graph, state).is_some() && grounded;
            let mut velocity = if alive {
                let free = movement_velocity(state)?;
                let mut percent = crate::aura::speed_percent(&state.auras)
                    * crate::surface::speed_percent(surface)
                    / 100;
                // A load slows nobody, but no boost carries it past the run speed (#1077).
                if carrying {
                    percent = percent.min(100);
                }
                if guarding {
                    percent = percent * crate::GUARD_SPEED_PERCENT / 100;
                }
                if swim_depth.is_some() {
                    percent = percent * crate::water::SWIM_SPEED_PERCENT / 100;
                }
                let mut steered = crate::aura::scale_velocity([free.x, free.z], percent);
                // The running action phases' authored movement (#413): the
                // controller is asked for a mix of intent and the velocity
                // contact let the body realize last step.
                if let Some(movement) = crate::action::movement_of(self.action_graph, state) {
                    let last = body.velocity();
                    steered = movement.velocity(steered, [last.x, last.z]);
                }
                // A running strike's lunge (#885) carries the body along its facing.
                if let Some(lunge) = self.lunge_velocity(player_id, state)? {
                    steered = lunge;
                }
                // Ice (#294): a slick surface limits how fast the velocity can change.
                if let Some(grip) = crate::surface::grip(surface) {
                    let last = body.velocity();
                    steered = crate::surface::slip([last.x, last.z], steered, grip);
                }
                let [x, z] = steered;
                Vec3i::new(x, 0, z)
            } else {
                Vec3i::ZERO
            };
            velocity.y = vertical_velocity;
            // A channel's current carries a swimmer, and a wader by depth (#667, Decision 17).
            if alive {
                let [x, z] = crate::water::current_push(
                    self.content.waters(),
                    [centre.x, centre.y - PLAYER_HALF_EXTENTS_UNITS[1], centre.z],
                );
                velocity.x = velocity.x.saturating_add(x);
                velocity.z = velocity.z.saturating_add(z);
            }
            // A follow-up's launch (#413), along the facing and upward.
            if let Some([forward, up]) = state.impulse.filter(|_| alive) {
                let (dx, dz) = trig::direction(state.facing);
                let forward = crate::action::speed_subunits(forward);
                let along = |unit| trig::checked_scale(forward, unit).unwrap_or(0);
                velocity.x = velocity.x.saturating_add(along(dx));
                velocity.z = velocity.z.saturating_add(along(dz));
                velocity.y = velocity.y.max(crate::action::speed_subunits(up));
            }
            self.world
                .set_velocity(body_id, velocity)
                .map_err(physics_error)?;
            self.world
                .set_weightless(body_id, swim_depth.is_some())
                .map_err(physics_error)?;
        }
        for player in self.players.values_mut() {
            player.impulse = None;
        }
        laden.dedup();
        for player_id in laden {
            self.notify(
                player_id,
                crate::ZoneEvent::Error {
                    code: crate::ErrorCode::HandsFull,
                    target: None,
                },
            );
        }
        for (player_id, traversal) in traversals {
            if let Some(player) = self.players.get_mut(&player_id) {
                player.traversal = traversal;
            }
        }
        // A roll cancels the roller's cast and repair, and any started mid-roll (#516).
        let mut rollers = Vec::new();
        for (player_id, roll) in rolls {
            if let Some(player) = self.players.get_mut(&player_id) {
                player.roll = roll;
                if roll.rolling().is_some() {
                    player.repairing = None;
                    rollers.push(player_id);
                }
            }
        }
        for player_id in rollers {
            self.interrupt_cast(EntityRef::Player(player_id), None, None);
        }
        Ok(())
    }

    /// Step 5: re-reads every body's position and moves index memberships
    /// only when a cell changes. Corpses, NPCs and standing props (#288) do not move.
    fn update_interest(&mut self) -> Result<(), ZoneError> {
        let content = &self.content;
        let creatures = &self.creatures;
        // An awake creature's body holds its decision bit (#1086); the asleep ones are
        // skipped below anyway, so only those creatures are read.
        let awake = creatures
            .active(crate::creature_store::Activity::Decide)
            .filter(|&id| {
                creatures.get(&id).is_some_and(CreatureState::is_alive)
                    && content
                        .creature_entry(id)
                        .is_some_and(|(_, template)| template.prop().is_none())
            })
            .map(EntityRef::Creature);
        let bodies = self
            .players
            .keys()
            .map(|&id| EntityRef::Player(id))
            .chain(awake);
        let minions = self.minions.keys().map(|&id| EntityRef::Minion(id));
        for entity in bodies.chain(minions) {
            let body = self
                .world
                .body(body_id(entity))
                .ok_or_else(|| ZoneError::new("unit physics body is missing"))?;
            // An asleep creature sat the step out where it was (#814).
            if body.asleep() {
                continue;
            }
            let position = body.position();
            self.interest_work.units_inspected += 1;
            if self.interest.move_if_needed(entity, position.x, position.z) {
                self.interest_work.bucket_moves += 1;
                self.interest_work.bucket_removes += 1;
                self.interest_work.bucket_inserts += 1;
            }
        }
        Ok(())
    }

    /// Derived state is reconstructed on recovery, never serialized.
    pub(crate) fn rebuild_interest(&mut self) -> Result<(), ZoneError> {
        let mut interest = InterestIndex::default();
        let mut inserted = 0;
        for npc in self.content.npcs() {
            let [x, z] = self.npc_feet(npc.id).unwrap_or(npc.position);
            interest.insert(EntityRef::Npc(npc.id), x, z);
            inserted += 1;
        }
        for &player_id in self.players.keys() {
            let entity = EntityRef::Player(player_id);
            let position = self
                .world
                .body(body_id(entity))
                .ok_or_else(|| ZoneError::new("player physics body is missing"))?
                .position();
            interest.insert(entity, position.x, position.z);
            inserted += 1;
            self.interest_work.units_inspected += 1;
        }
        for (&id, creature) in &self.creatures {
            let entity = EntityRef::Creature(id);
            if let Some(position) = self.creature_position(id, creature)? {
                interest.insert(entity, position[0], position[2]);
                inserted += 1;
            }
        }
        for &id in self.minions.keys() {
            let entity = EntityRef::Minion(id);
            let position = self.unit_position(entity)?;
            interest.insert(entity, position[0], position[2]);
            inserted += 1;
        }
        self.interest = interest;
        self.interest_work.full_rebuilds += 1;
        self.interest_work.bucket_inserts += inserted;
        Ok(())
    }

    /// Places a unit in the index at `(x, z)`, whether or not it was indexed.
    pub(crate) fn place_in_interest(&mut self, entity: EntityRef, x: i32, z: i32) {
        if self.interest.move_if_needed(entity, x, z) {
            self.interest_work.bucket_inserts += 1;
        }
    }

    /// The authored ground's height at `(x, z)` in whole units (#159); 0 on flat content.
    pub(crate) fn ground_y(&self, x: i32, z: i32) -> i32 {
        // Heights stay within ±100 m, far inside `i32`.
        #[allow(clippy::cast_possible_truncation)]
        self.world
            .terrain_height(x, z)
            .map_or(0, |height| height.round() as i32)
    }

    /// The body centre height (units) at which a unit with `half_extents` stands at `(x, z)`
    /// on the authored ground, not in it (#534): its capsule's round bottom rests on the
    /// highest ground under its footprint, so a creature on a hillside is never spawned into
    /// the slope. On flat ground it is `ground_y + half_extents[1]`.
    #[must_use]
    pub fn stand_y(&self, x: i32, z: i32, half_extents: [i32; 3]) -> i32 {
        let [hx, hy, hz] = half_extents;
        let radius = f64::from(crate::physics::capsule_dimensions(Vec3i::new(hx, hy, hz)).1);
        // As `ground_y`: no terrain (a flat cell) is the floor at 0.
        let ground = |x: i32, z: i32| f64::from(self.world.terrain_height(x, z).unwrap_or(0.0));
        let mut feet = ground(x, z);
        // Rings at a quarter, half, three quarters and the whole radius, eight directions each:
        // the bottom hemisphere is `radius - sqrt(radius² - d²)` above its lowest point there.
        for ring in 1..=4_i32 {
            let d = radius * f64::from(ring) / 4.0;
            let lift = radius - (radius * radius - d * d).max(0.0).sqrt();
            #[allow(clippy::cast_possible_truncation)]
            let (straight, diagonal) = (
                d.round() as i32,
                (d * std::f64::consts::FRAC_1_SQRT_2).round() as i32,
            );
            for (dx, dz) in [
                (straight, 0),
                (-straight, 0),
                (0, straight),
                (0, -straight),
                (diagonal, diagonal),
                (diagonal, -diagonal),
                (-diagonal, diagonal),
                (-diagonal, -diagonal),
            ] {
                feet = feet.max(ground(x + dx, z + dz) - lift);
            }
        }
        // Heights stay within ±100 m, far inside `i32`; a flat sample reads within 0.01 of its
        // whole unit.
        #[allow(clippy::cast_possible_truncation)]
        let feet = (feet - 0.01).ceil() as i32;
        feet + hy
    }

    /// Whether a unit with `half_extents` standing with its body centre at `centre` (units) is a
    /// unit clear of every body with an extent all round: content boxes, props, door leaves and
    /// other units. The ground (floors and the terrain) is not in the way (#534).
    #[must_use]
    pub fn stands_clear(&self, centre: [i32; 3], half_extents: [i32; 3]) -> bool {
        let [hx, hy, hz] = half_extents;
        let solid = |hit| {
            self.world
                .body(hit)
                .is_some_and(|body| body.half_extents() != Vec3i::ZERO)
        };
        // A capsule a unit wider and taller all round, its feet a unit up, swept a hair.
        #[allow(clippy::cast_precision_loss)]
        let origin = [centre[0] as f32, (centre[1] + 2) as f32, centre[2] as f32];
        self.world
            .cast_shape(
                origin,
                Vec3i::new(hx + 1, hy + 1, hz + 1),
                [0.0, 0.01, 0.0],
                solid,
            )
            .is_none()
    }

    /// A player's exact body centre (units) and the velocity (units per tick)
    /// contact let it realize in the last step, as canonical snapshots carry
    /// them; read-only, for the Action Lab's movement inspector (#413).
    #[must_use]
    pub fn player_motion(&self, player_id: PlayerId) -> Option<([f32; 3], [f32; 3])> {
        let body = self.world.body(body_id(EntityRef::Player(player_id)))?;
        Some((body.exact_position(), body.exact_velocity()))
    }

    /// The body position of a player.
    pub(crate) fn player_position(&self, player_id: PlayerId) -> Result<Vec3i, ZoneError> {
        Ok(self
            .world
            .body(body_id(EntityRef::Player(player_id)))
            .ok_or_else(|| ZoneError::new("player physics body is missing"))?
            .position())
    }

    pub(crate) fn available_spawn_slot(&self) -> Option<u16> {
        (0..MAX_PLAYERS_PER_ZONE)
            .find(|candidate| {
                self.players
                    .values()
                    .all(|state| usize::from(state.spawn_slot) != *candidate)
            })
            .and_then(|slot| u16::try_from(slot).ok())
    }

    /// Body centre for a slot of the content's validated spawn grid; feet
    /// rest on y = 0, the top of flat walkable ground.
    pub(crate) fn spawn_position(&self, spawn_slot: u16) -> Result<Vec3i, ZoneError> {
        let feet = self
            .definition()
            .spawn_grid()
            .feet(spawn_slot)
            .ok_or_else(|| ZoneError::new("player spawn slot is out of range"))?;
        Ok(Vec3i::new(feet[0], PLAYER_HALF_EXTENTS_UNITS[1], feet[2]))
    }
}

pub(crate) const fn vector(value: [i32; 3]) -> Vec3i {
    Vec3i::new(value[0], value[1], value[2])
}

/// Whether a body centre lies strictly inside the world-limit slabs, with
/// room for a player body.
pub(crate) fn within_world_limits(position: Vec3i) -> bool {
    [
        (position.x, PLAYER_HALF_EXTENTS_UNITS[0]),
        (position.y, PLAYER_HALF_EXTENTS_UNITS[1]),
        (position.z, PLAYER_HALF_EXTENTS_UNITS[2]),
    ]
    .into_iter()
    .all(|(component, half)| component.abs() <= MAX_CONTENT_COORDINATE_UNITS - half)
}

/// Six fixed slabs whose inner faces close the cube `±MAX_CONTENT_COORDINATE_UNITS`.
/// Content walls are the gameplay boundary; these bodies are not content. They
/// keep every body admitted at a spawn slot inside the range that player
/// projections encode, whatever the content's walls or gravity.
fn world_limit_bodies() -> impl Iterator<Item = RigidBody> {
    let limit = MAX_CONTENT_COORDINATE_UNITS;
    let half_thickness = WORLD_LIMIT_THICKNESS_UNITS / 2;
    let span = limit + WORLD_LIMIT_THICKNESS_UNITS;
    let slabs = (0..3_usize).flat_map(|axis| [(axis, -1), (axis, 1)]);
    (WORLD_LIMIT_BODY_BASE..)
        .zip(slabs)
        .map(move |(id, (axis, side))| {
            let mut position = [0; 3];
            position[axis] = side * (limit + half_thickness);
            let mut half_extents = [span; 3];
            half_extents[axis] = half_thickness;
            // The bottom limit is a floor half-space at its top face.
            if (axis, side) == (1, -1) {
                return RigidBody::floor(crate::physics::BodyId(id), -limit);
            }
            RigidBody::fixed(
                crate::physics::BodyId(id),
                vector(position),
                vector(half_extents),
            )
        })
}

/// The walk intent `state` holds: its facing, forward and strafe, and a pending jump.
fn walk_intent(state: &PlayerState) -> walk::Intent {
    walk::Intent {
        forward: state.forward.get(),
        strafe: state.strafe.get(),
        yaw: state.facing,
        jump: state.jump_pending,
    }
}

/// Horizontal controller velocity: run or backpedal speed along [`movement_yaw`]
/// (`sim_movement::walk`, #749). Zero intent yields zero horizontal velocity.
fn movement_velocity(state: &PlayerState) -> Result<Vec3i, ZoneError> {
    let [x, z] = walk::walk_velocity(walk_intent(state), walk::Speeds::RUN)
        .ok_or_else(|| ZoneError::new("movement velocity overflow"))?;
    Ok(Vec3i::new(x, 0, z))
}

/// The yaw held movement intent moves along: `facing` plus one of eight multiples of 45°, or
/// `None` without intent.
fn movement_yaw(state: &PlayerState) -> Option<u16> {
    walk::move_yaw(walk_intent(state))
}

/// A thin probe directly under the feet, inset by one unit horizontally so
/// touching a wall is not ground. The physics query counts touching bodies;
/// any body other than the player itself supports a jump.
pub(crate) fn is_grounded(world: &World, body: &RigidBody) -> Result<bool, ZoneError> {
    Ok(world.grounded(body.id()))
}

pub(crate) fn saturate_i8(value: i32) -> i8 {
    i8::try_from(value).unwrap_or(if value < 0 { i8::MIN } else { i8::MAX })
}

pub(crate) fn physics_error(error: crate::physics::PhysicsError) -> ZoneError {
    ZoneError::new(error.to_string())
}

/// Whether `collider` is the flat ground the spawn grid stands on (feet at `spawn`): its top at
/// y = 0 under the spawn. It becomes the floor half-space, and the terrain shares its ID.
fn is_ground(collider: &crate::StaticCollider, spawn: [i32; 3]) -> bool {
    let [x, y, z] = collider.position;
    let [hx, hy, hz] = collider.half_extents;
    y + hy == 0 && (x - spawn[0]).abs() <= hx && (z - spawn[2]).abs() <= hz
}

impl ZoneSimulation {
    /// The ground's body (#57): the floor half-space and the terrain rising from it.
    pub(crate) fn ground_body(&self) -> Option<crate::physics::BodyId> {
        let definition = self.content.definition();
        let spawn = definition.spawn_grid().feet(0).unwrap_or([0; 3]);
        definition
            .colliders()
            .iter()
            .find(|collider| is_ground(collider, spawn))
            .map(|collider| crate::physics::BodyId(u64::from(collider.id)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::EntityKind;

    #[test]
    fn movement_is_driven_through_the_shared_physics() {
        let mut zone = ZoneSimulation::new(ZoneId::new(7));
        zone.add_player(1).unwrap();
        let before = zone.snapshot().unwrap().players[0].position;

        zone.apply_command(
            1,
            1,
            ZoneCommand::Move {
                forward: 1,
                strafe: 0,
                facing: YAW_QUARTER_TURN,
            },
        )
        .unwrap();
        for _ in 0..10 {
            zone.advance_tick().unwrap();
        }

        let after = zone.snapshot().unwrap().players[0].position;
        assert!(after[0] > before[0]);
        assert_eq!(after[2], before[2]);
    }

    #[test]
    fn configured_capacity_has_unique_spawn_positions() {
        use std::collections::BTreeSet;

        let mut zone = ZoneSimulation::new(ZoneId::new(1));
        for index in 0..MAX_PLAYERS_PER_ZONE {
            let player_id = u32::try_from(index)
                .expect("configured capacity fits player id")
                .checked_mul(1_024)
                .and_then(|value| value.checked_add(1))
                .expect("test player id fits u32");
            zone.add_player(player_id).unwrap();
        }

        let positions = zone
            .snapshot()
            .unwrap()
            .players
            .into_iter()
            .map(|player| {
                let [x, _, z] = crate::physics::to_units(player.position);
                (x, z)
            })
            .collect::<BTreeSet<_>>();

        assert_eq!(positions.len(), MAX_PLAYERS_PER_ZONE);
    }

    #[test]
    fn canonical_snapshot_restores_movement_sequence_and_spawn_allocation() {
        let mut original = ZoneSimulation::new(ZoneId::new(1));
        original.add_player(1).unwrap();
        original
            .apply_command(
                1,
                6,
                ZoneCommand::Move {
                    forward: 1,
                    strafe: -1,
                    facing: 1_234,
                },
            )
            .unwrap();
        original.apply_command(1, 7, ZoneCommand::Jump).unwrap();

        let snapshot = original.snapshot().unwrap();
        assert_eq!(snapshot.players[0].facing, 1_234);
        assert_eq!(snapshot.players[0].forward, 1);
        assert_eq!(snapshot.players[0].strafe, -1);
        assert!(snapshot.players[0].jump_pending);
        assert_eq!(snapshot.players[0].last_sequence, 7);

        let mut recovered =
            ZoneSimulation::from_snapshot(snapshot, Arc::clone(original.content())).unwrap();
        assert_eq!(
            recovered
                .apply_command(1, 7, ZoneCommand::Jump)
                .unwrap_err()
                .message(),
            "command sequence is stale"
        );

        original.advance_tick().unwrap();
        recovered.advance_tick().unwrap();
        assert_eq!(recovered.snapshot().unwrap(), original.snapshot().unwrap());

        recovered.add_player(1_025).unwrap();
        let recovered = recovered.snapshot().unwrap();
        assert_ne!(
            recovered.players[0].spawn_slot,
            recovered.players[1].spawn_slot
        );
    }

    #[test]
    fn player_projection_fails_closed_when_physics_state_is_incomplete() {
        let mut zone = ZoneSimulation::new(ZoneId::new(1));
        zone.add_player(1).unwrap();
        zone.add_player(2).unwrap();
        zone.world.remove_body(body_id(EntityRef::Player(2)));

        let error = zone.snapshot_for_player(1).unwrap_err();

        assert_eq!(error.message(), "player physics body is missing");
    }

    #[test]
    fn stale_commands_fail_closed() {
        let mut zone = ZoneSimulation::new(ZoneId::new(1));
        zone.add_player(1).unwrap();
        let run = ZoneCommand::Move {
            forward: 1,
            strafe: 0,
            facing: 0,
        };
        zone.apply_command(1, 5, run).unwrap();

        for (sequence, command) in [
            (5, run),
            (4, ZoneCommand::Jump),
            (5, ZoneCommand::Jump),
            (0, ZoneCommand::Jump),
            (5, ZoneCommand::Swing { facing: 0 }),
            (2, ZoneCommand::SelectTarget(None)),
        ] {
            let error = zone.apply_command(1, sequence, command).unwrap_err();
            assert_eq!(error.message(), "command sequence is stale");
        }
        let player = &zone.snapshot().unwrap().players[0];
        assert!(!player.jump_pending, "rejected commands change nothing");
        assert!(player.combat.intents.is_empty());
        assert_eq!(player.last_sequence, 5);
    }

    #[test]
    fn a_sequence_far_ahead_fails_closed_and_keeps_the_player_usable() {
        let mut zone = ZoneSimulation::new(ZoneId::new(1));
        zone.add_player(1).unwrap();
        zone.apply_command(1, 5, ZoneCommand::Jump).unwrap();

        // A garbage maximal sequence would leave no later one (#1151).
        for sequence in [u32::MAX, 5 + MAX_SEQUENCE_GAP + 1] {
            let error = zone
                .apply_command(1, sequence, ZoneCommand::SelectTarget(None))
                .unwrap_err();
            assert_eq!(error.message(), "command sequence is too far ahead");
        }
        let player = &zone.snapshot().unwrap().players[0];
        assert_eq!(
            player.last_sequence, 5,
            "a refused sequence is not consumed"
        );
        assert!(player.combat.intents.is_empty());

        // The client's next sequences still apply, up to the full gap.
        zone.apply_command(1, 6, ZoneCommand::CancelCast).unwrap();
        zone.apply_command(1, 6 + MAX_SEQUENCE_GAP, ZoneCommand::CancelCast)
            .unwrap();
    }

    #[test]
    fn a_refused_physics_query_fails_the_tick_instead_of_panicking() {
        let mut zone = ZoneSimulation::new(ZoneId::new(1));
        zone.add_player(1).unwrap();
        zone.advance_tick().unwrap();
        assert_eq!(
            zone.world
                .cast_ray([f32::NAN, 0.0, 0.0], [1.0, 0.0, 0.0], |_| true),
            None
        );
        let error = zone.advance_tick().unwrap_err();
        assert!(error.message().contains("ray query"), "{}", error.message());
    }

    #[test]
    fn excess_intents_are_dropped_and_reported_without_a_session_error() {
        let mut zone = ZoneSimulation::new(ZoneId::new(1));
        zone.add_player(1).unwrap();
        for sequence in 1..=u32::try_from(MAX_PENDING_INTENTS).unwrap() {
            zone.apply_command(1, sequence, ZoneCommand::CancelCast)
                .unwrap();
        }
        // A well-formed intent beyond the bound is an outcome, not an error.
        zone.apply_command(1, 100, ZoneCommand::SelectTarget(None))
            .unwrap();
        zone.apply_command(1, 101, ZoneCommand::ReleaseSpirit)
            .unwrap();
        let player = &zone.snapshot().unwrap().players[0];
        assert_eq!(player.last_sequence, 101, "the sequences are consumed");
        assert_eq!(
            player.combat.intents,
            [crate::QueuedIntent::from(PlayerIntent::CancelCast); MAX_PENDING_INTENTS],
            "the excess intents are dropped"
        );
        assert!(player.combat.intents_dropped);
        assert_eq!(
            zone.apply_command(1, 101, ZoneCommand::CancelCast)
                .unwrap_err()
                .message(),
            "command sequence is stale"
        );

        zone.advance_tick().unwrap();
        let player = &zone.snapshot().unwrap().players[0];
        assert!(player.combat.intents.is_empty() && !player.combat.intents_dropped);
        assert_eq!(
            zone.snapshot_for_player(1).unwrap().events,
            [ZoneEvent::Error {
                code: crate::ErrorCode::TooManyIntents,
                target: None,
            }],
            "the next tick reports the drop once"
        );
        zone.apply_command(1, 102, ZoneCommand::CancelCast).unwrap();
        zone.advance_tick().unwrap();
        assert!(zone.snapshot_for_player(1).unwrap().events.is_empty());
    }

    #[test]
    fn a_dropped_intent_survives_recovery_and_needs_a_full_queue() {
        let mut original = ZoneSimulation::new(ZoneId::new(1));
        original.add_player(1).unwrap();
        for sequence in 1..=u32::try_from(MAX_PENDING_INTENTS).unwrap() + 1 {
            original
                .apply_command(1, sequence, ZoneCommand::CancelCast)
                .unwrap();
        }
        let checkpoint = original.snapshot().unwrap();
        let mut recovered =
            ZoneSimulation::from_snapshot(checkpoint.clone(), Arc::clone(original.content()))
                .unwrap();
        original.advance_tick().unwrap();
        recovered.advance_tick().unwrap();
        assert_eq!(recovered.snapshot().unwrap(), original.snapshot().unwrap());

        let mut inconsistent = checkpoint;
        inconsistent.players[0].combat.intents.pop();
        assert_eq!(
            ZoneSimulation::from_snapshot(inconsistent, Arc::clone(original.content()))
                .err()
                .unwrap()
                .message(),
            "only a full intent queue drops intents"
        );
    }

    #[test]
    fn malformed_movement_fails_without_consuming_the_sequence() {
        let mut zone = ZoneSimulation::new(ZoneId::new(1));
        zone.add_player(1).unwrap();
        for (forward, strafe) in [(2, 0), (0, -2), (i8::MIN, 1), (1, i8::MAX)] {
            let error = zone
                .apply_command(
                    1,
                    3,
                    ZoneCommand::Move {
                        forward,
                        strafe,
                        facing: 9,
                    },
                )
                .unwrap_err();
            assert_eq!(
                error.message(),
                "movement components must be between -1 and 1"
            );
        }
        let player = &zone.snapshot().unwrap().players[0];
        assert_eq!(
            (
                player.facing,
                player.forward,
                player.strafe,
                player.last_sequence
            ),
            (0, 0, 0, 0)
        );
        zone.apply_command(
            1,
            3,
            ZoneCommand::Move {
                forward: -1,
                strafe: 1,
                facing: 9,
            },
        )
        .unwrap();
    }

    #[test]
    fn recovery_rejects_out_of_range_movement_intent() {
        let mut zone = ZoneSimulation::new(ZoneId::new(1));
        zone.add_player(1).unwrap();
        let valid = zone.snapshot().unwrap();
        for (forward, strafe) in [(2, 0), (0, -2)] {
            let mut invalid = valid.clone();
            invalid.players[0].forward = forward;
            invalid.players[0].strafe = strafe;
            assert_eq!(
                ZoneSimulation::from_snapshot(invalid, Arc::clone(zone.content()))
                    .err()
                    .unwrap()
                    .message(),
                "movement components must be between -1 and 1"
            );
        }
        let mut invalid_slot = valid;
        invalid_slot.players[0].spawn_slot = u16::try_from(MAX_PLAYERS_PER_ZONE).unwrap();
        assert!(ZoneSimulation::from_snapshot(invalid_slot, Arc::clone(zone.content())).is_err());
    }

    #[test]
    fn player_snapshot_applies_zone_owned_interest_policy() {
        let mut zone = ZoneSimulation::new(ZoneId::new(1));
        // The default grid spaces one row of spawn slots 200 units apart along
        // +X: player 24 stands 4 600 units from player 1, beyond 45 m.
        for player_id in 1..=24 {
            zone.add_player(player_id).unwrap();
        }

        let canonical = zone.snapshot().unwrap();
        let visible = zone.snapshot_for_player(1).unwrap();

        assert_eq!(canonical.players.len(), 24);
        assert_eq!(visible.viewer_id, 1);
        assert_eq!(visible.entities.len(), 23);
        assert!(
            visible
                .entities
                .iter()
                .all(|entity| entity.kind == EntityKind::Player && entity.id != 24)
        );
        // The viewer leads, then nearer units; equal distances order by ID.
        let middle = zone.project_for_player(3).unwrap();
        let order: Vec<_> = middle.snapshot.entities.iter().map(|e| e.id).collect();
        assert_eq!(order[..5], [3, 2, 4, 1, 5]);
        assert_eq!(middle.stats.relevant, 24);
    }

    #[test]
    fn projection_carries_facing_and_saturates_presentation_velocity() {
        let mut zone = ZoneSimulation::new(ZoneId::new(1));
        zone.add_player(1).unwrap();
        zone.add_player(2).unwrap();
        zone.apply_command(
            2,
            1,
            ZoneCommand::Move {
                forward: 0,
                strafe: 0,
                facing: 40_000,
            },
        )
        .unwrap();
        let mut canonical = zone.snapshot().unwrap();
        canonical.players[0].velocity = [70_000.0, -70_000.0, 32_767.0];
        let zone = ZoneSimulation::from_snapshot(canonical, Arc::clone(zone.content())).unwrap();

        let visible = zone.snapshot_for_player(2).unwrap();

        assert_eq!(visible.viewer_id, 2);
        assert_eq!(visible.acknowledged_sequence, 1);
        let [viewer, other] = &visible.entities[..] else {
            panic!("both players are relevant");
        };
        assert_eq!((viewer.id, viewer.facing), (2, 40_000), "the viewer leads");
        assert_eq!(other.id, 1);
        assert_eq!(other.velocity, [i8::MAX, i8::MIN, i8::MAX]);
        assert_eq!(other.facing, 0);
        assert_eq!(
            zone.snapshot().unwrap().players[0].velocity,
            [70_000.0, -70_000.0, 32_767.0],
            "canonical state keeps exact velocity"
        );
    }
}
