//! The zone simulation: construction, admission, commands, player movement
//! and the fixed tick order. Combat, creature AI, projections and canonical
//! snapshots are further `impl ZoneSimulation` blocks in their own modules.

use std::collections::BTreeMap;
use std::sync::Arc;

use crate::physics::{RigidBody, Vec3i, World};

use crate::creature::CreatureState;
use crate::entity::{WORLD_LIMIT_BODY_BASE, body_id};
use crate::interest::InterestIndex;
use crate::rng::ZoneRng;
use crate::snapshot::PlayerIntent;
use crate::trig::{self, YAW_EIGHTH_TURN, YAW_QUARTER_TURN};
use crate::unit::{PLAYER_START_LEVEL, player_max_health};
use crate::{
    BACKPEDAL_SPEED_SUBUNITS_PER_TICK, CreatureId, EntityRef, InterestMaintenanceStats,
    JUMP_VELOCITY_SUBUNITS_PER_TICK, MAX_CONTENT_COORDINATE_UNITS, MAX_PLAYERS_PER_ZONE,
    PLAYER_HALF_EXTENTS_UNITS, PlayerId, RUN_SPEED_SUBUNITS_PER_TICK, ZoneCommand, ZoneContent,
    ZoneDefinition, ZoneError, ZoneEvent, ZoneId,
};

/// Discrete intents (targeting, attacking, releasing) a player may queue
/// between two ticks. Further intents are accepted and dropped, and the next
/// tick reports [`crate::ErrorCode::TooManyIntents`] once.
pub const MAX_PENDING_INTENTS: usize = 16;

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
    /// The destructible piece the player is repairing (#305).
    pub(crate) repairing: Option<crate::PieceId>,
    pub(crate) last_sequence: u32,
    pub(crate) spawn_slot: u16,
    pub(crate) level: u8,
    pub(crate) experience: u32,
    pub(crate) copper: u32,
    pub(crate) inventory: crate::Inventory,
    pub(crate) inventory_revision: u64,
    pub(crate) inventory_changed_at: u64,
    /// Equipped items; they share the bag's revision and sheet.
    pub(crate) equipment: crate::Equipment,
    /// Zero means dead.
    pub(crate) health: u32,
    pub(crate) target: Option<EntityRef>,
    pub(crate) auto_attack: bool,
    /// Ticks until the next swing may happen.
    pub(crate) swing_timer: u16,
    /// Ticks the player stays in combat after dealing or taking an attack.
    pub(crate) combat_timer: u16,
    /// Ticks spent out of combat, driving regeneration.
    pub(crate) calm_ticks: u16,
    /// Ticks until another out-of-range error may be reported.
    pub(crate) error_cooldown: u16,
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
    /// Running cooldowns ordered by ability.
    pub(crate) cooldowns: Vec<crate::Cooldown>,
    pub(crate) cast: Option<crate::CastState>,
    /// Auras in slot order.
    pub(crate) auras: Vec<crate::Aura>,
    /// The first tick the player may speak again.
    pub(crate) chat_ready_at: u64,
    /// Chat lines heard this tick.
    pub(crate) chat: Vec<crate::ChatLine>,
    /// Active and turned-in quests.
    pub(crate) quests: crate::QuestLog,
    /// The tick the quest log last changed; its sheet is sent then.
    pub(crate) quests_changed_at: u64,
}

impl PlayerState {
    pub(crate) fn new(spawn_slot: u16, tick: u64) -> Self {
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
            impulse: None,
            authored: None,
            holds: crate::action::authored::Holds::default(),
            combo: None,
            strike_recovery: 0,
            repairing: None,
            last_sequence: 0,
            spawn_slot,
            level: PLAYER_START_LEVEL,
            experience: 0,
            copper: 0,
            inventory: crate::Inventory::starter(),
            inventory_revision: 1,
            inventory_changed_at: tick,
            equipment: crate::Equipment::default(),
            health: player_max_health(PLAYER_START_LEVEL),
            target: None,
            auto_attack: false,
            swing_timer: 0,
            combat_timer: 0,
            calm_ticks: 0,
            error_cooldown: 0,
            intents: Vec::new(),
            intents_dropped: false,
            last_observed: 0,
            events: Vec::new(),
            class: None,
            resource: crate::class::ResourceClock::default(),
            global_cooldown: 0,
            cooldowns: Vec::new(),
            cast: None,
            auras: Vec::new(),
            chat_ready_at: 0,
            chat: Vec::new(),
            quests: crate::QuestLog::default(),
            quests_changed_at: tick,
        }
    }

    pub(crate) const fn is_alive(&self) -> bool {
        self.health > 0
    }

    /// Level health plus the equipped stamina's bonus.
    pub(crate) fn max_health(&self) -> u32 {
        player_max_health(self.level) + self.equipment.totals().bonus_health()
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
        [low + bonus, high + bonus]
    }
}

/// Diagnostic work for the latest attempted tick. Creation and recovery start
/// at zero. These counters never enter canonical state or player projections.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ZoneTickWork {
    /// Living creatures whose AI decision was evaluated, including idle units.
    pub creature_ai_evaluations: usize,
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
    pub(crate) players: BTreeMap<PlayerId, PlayerState>,
    pub(crate) creatures: BTreeMap<CreatureId, CreatureState>,
    pub(crate) interest: InterestIndex,
    pub(crate) interest_work: InterestMaintenanceStats,
    pub(crate) tick_work: ZoneTickWork,
    /// Door states by ID; absent means closed and never moved (#158).
    pub(crate) doors: BTreeMap<crate::DoorId, crate::door::DoorState>,
    /// Flying and stuck projectiles by ID (#286).
    pub(crate) projectiles: BTreeMap<u32, crate::Projectile>,
    pub(crate) next_projectile: u32,
    /// Forced moves in progress and the immunity after them (#287).
    pub(crate) forced: BTreeMap<EntityRef, crate::ForcedState>,
    /// Pieces of broken props by ID (#288).
    pub(crate) debris: BTreeMap<u32, crate::Debris>,
    pub(crate) next_debris: u32,
    /// Destructible pieces' health and last change (#305).
    pub(crate) pieces: BTreeMap<crate::PieceId, crate::structure::PieceState>,
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
    pub(crate) minions: BTreeMap<crate::MinionId, crate::Minion>,
    pub(crate) next_minion: u32,
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
        let mut zone = Self::without_creatures(zone_id, content)?;
        let spawns: Vec<_> = zone
            .content
            .creature_spawns()
            .iter()
            .map(|spawn| spawn.id)
            .collect();
        for id in spawns {
            zone.spawn_creature(id)?;
        }
        Ok(zone)
    }

    /// Static world, NPC bodies and a freshly seeded RNG; recovery adds the
    /// recorded units on top.
    pub(crate) fn without_creatures(
        zone_id: ZoneId,
        content: Arc<ZoneContent>,
    ) -> Result<Self, ZoneError> {
        let definition = content.definition();
        let gravity = definition.gravity();
        let mut world = World::new(Vec3i::new(gravity[0], gravity[1], gravity[2]));
        let spawn = definition.spawn_grid().feet(0).unwrap_or([0; 3]);
        for collider in definition.colliders() {
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
            world
                .add_body(RigidBody::fixed(
                    crate::physics::BodyId(u64::from(collider.id)),
                    vector(collider.position),
                    vector(collider.half_extents),
                ))
                .map_err(physics_error)?;
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
        Ok(Self {
            zone_id,
            tick: 0,
            world,
            rng: ZoneRng::seeded(content.rng_seed(), zone_id),
            loot_rng: ZoneRng::seeded(content.rng_seed() ^ 0x6c6f_6f74_2f76_3031, zone_id),
            content,
            players: BTreeMap::new(),
            creatures: BTreeMap::new(),
            interest,
            interest_work: InterestMaintenanceStats::default(),
            tick_work: ZoneTickWork::default(),
            doors: BTreeMap::new(),
            projectiles: BTreeMap::new(),
            next_projectile: 1,
            forced: BTreeMap::new(),
            debris: BTreeMap::new(),
            next_debris: 1,
            pieces,
            nav_cache: BTreeMap::new(),
            prop_motion: BTreeMap::new(),
            action_graph: &crate::action::BASELINE,
            resolving_late: 0,
            minions: BTreeMap::new(),
            next_minion: 1,
            rewards: None,
        })
    }

    /// The immutable content this zone simulates.
    #[must_use]
    pub const fn content(&self) -> &Arc<ZoneContent> {
        &self.content
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
        self.players
            .insert(player_id, PlayerState::new(spawn_slot, self.tick));
        Ok(())
    }

    /// Removes the player's unit at once: its body, its place on every
    /// threat table and every other player's selection of it.
    pub fn remove_player(&mut self, player_id: PlayerId) -> bool {
        if self.players.remove(&player_id).is_none() {
            return false;
        }
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
        for creature in self.creatures.values_mut() {
            creature.forget(entity);
            // A cast at a departed player resolves against nobody.
            if creature
                .cast
                .is_some_and(|cast| cast.target == Some(entity))
            {
                creature.cast = None;
            }
            if creature.tapped_by == Some(player_id) {
                creature.tapped_by = None;
                creature.loot = None;
            }
        }
        for player in self.players.values_mut() {
            if player.cast.is_some_and(|cast| cast.target == Some(entity)) {
                player.cast = None;
            }
            if player.combo.is_some_and(|combo| combo.target == entity) {
                player.combo = None;
            }

            if player.target == Some(entity) {
                player.target = None;
                player.auto_attack = false;
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
            // Queued so a guard pressed after a strike stays raised (5.2.37).
            ZoneCommand::Guard { lane } => Some(PlayerIntent::Guard(lane)),
            ZoneCommand::UseDoor { door, open } => Some(PlayerIntent::UseDoor { door, open }),
            ZoneCommand::Repair { piece } => Some(PlayerIntent::Repair(piece)),
            ZoneCommand::SelectTarget(target) => Some(PlayerIntent::SelectTarget(target)),
            ZoneCommand::StartAttack => Some(PlayerIntent::StartAttack),
            ZoneCommand::Loot(claim) => Some(PlayerIntent::Loot(claim)),
            ZoneCommand::StopAttack => Some(PlayerIntent::StopAttack),
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
            ZoneCommand::UseAbility { ability, target } => {
                Some(PlayerIntent::UseAbility { ability, target })
            }
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
    ///    after movement, damage, threat, deaths, tapping (minions after creatures);
    /// 7. minions whose time ran out crumble, then health and class-resource regeneration,
    ///    corpse despawns and respawns in ID order;
    /// 8. talk and explore quest objectives of living players in player-ID
    ///    order;
    /// 9. the tick counter.
    ///
    /// Events of the previous tick are cleared first, so each player's queue
    /// holds exactly this tick's events until the next tick starts.
    pub fn advance_tick(&mut self) -> Result<(), ZoneError> {
        let now = self
            .tick
            .checked_add(1)
            .ok_or_else(|| ZoneError::new("zone tick overflow"))?;
        if let Some(rewards) = &mut self.rewards {
            rewards.begin_tick();
        }
        self.tick_work = ZoneTickWork::default();
        for player in self.players.values_mut() {
            player.events.clear();
            player.chat.clear();
        }
        self.consume_intents()?;
        self.decide_creatures()?;
        self.decide_minions()?;
        self.drive_players()?;
        self.shove_with_large_creatures()?;
        self.apply_forced_moves()?;
        self.schedule_platforms(now, false)?;
        self.tick_work.physics = Some(self.world.step().map_err(physics_error)?);
        self.settle_forced_moves();
        self.advance_debris();
        self.advance_movable_props()?;
        self.update_landings()?;
        self.update_breath()?;
        // A jump intent is consumed by the tick that evaluated it, grounded or not.
        for player in self.players.values_mut() {
            player.jump_pending = false;
        }
        self.update_interest()?;
        self.advance_casts(now)?;
        self.advance_projectiles(now)?;
        self.advance_auras(now)?;
        self.resolve_combat(now)?;
        self.update_timers(now)?;
        self.regrow_pieces(now)?;
        self.advance_combos();
        self.advance_authored();
        self.advance_quest_objectives(now)?;
        self.tick = now;
        // Routine NPCs stand where the new tick's time of day puts them (#158).
        self.place_routine_npcs(true)?;
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
            }
        }
        let probe_ground = self.ground_body();
        let mut traversals = Vec::with_capacity(self.players.len());
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
            // A climb or mantle (#490) moves the body weightless along the zone's velocity.
            let step = if alive && swim_depth.is_none() {
                crate::climb::decide(
                    &probe,
                    body.exact_position(),
                    state.traversal,
                    crate::climb::Intent {
                        forward: i32::from(state.forward.get()),
                        strafe: i32::from(state.strafe.get()),
                        jump: state.jump_pending,
                        facing: state.facing,
                        grounded: is_grounded(&self.world, body)?,
                        stunned,
                    },
                )
            } else {
                crate::climb::Step::FREE
            };
            traversals.push((player_id, step.traversal));
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
            } else if alive && !stunned && state.jump_pending && is_grounded(&self.world, body)? {
                JUMP_VELOCITY_SUBUNITS_PER_TICK
            } else if state.traversal.holds() {
                // Letting go of a face or landing a mantle keeps no upward climb.
                body.velocity().y.min(0)
            } else {
                body.velocity().y
            };
            // A protecting guard slows movement (#156): ready and grounded.
            let guarding = crate::guard::guard_ready(self.action_graph, state).is_some()
                && is_grounded(&self.world, body)?;
            let mut velocity = if alive {
                let free = movement_velocity(state)?;
                let mut percent = crate::aura::speed_percent(&state.auras);
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
                let [x, z] = steered;
                Vec3i::new(x, 0, z)
            } else {
                Vec3i::ZERO
            };
            velocity.y = vertical_velocity;
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
        for (player_id, traversal) in traversals {
            if let Some(player) = self.players.get_mut(&player_id) {
                player.traversal = traversal;
            }
        }
        Ok(())
    }

    /// Step 5: re-reads every body's position and moves index memberships
    /// only when a cell changes. Corpses, NPCs and standing props (#288) do not move.
    fn update_interest(&mut self) -> Result<(), ZoneError> {
        let content = &self.content;
        let bodies = self.players.keys().map(|&id| EntityRef::Player(id)).chain(
            self.creatures
                .iter()
                .filter(|&(&id, creature)| {
                    creature.is_alive()
                        && content
                            .creature_spawn(id)
                            .and_then(|spawn| content.creature_template(spawn.template))
                            .is_some_and(|template| template.prop().is_none())
                })
                .map(|(&id, _)| EntityRef::Creature(id)),
        );
        let minions = self.minions.keys().map(|&id| EntityRef::Minion(id));
        for entity in bodies.chain(minions) {
            let position = self
                .world
                .body(body_id(entity))
                .ok_or_else(|| ZoneError::new("unit physics body is missing"))?
                .position();
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

/// Horizontal controller velocity: `speed × direction(facing + local offset)`,
/// where the offset is one of eight multiples of 45°. The character's right is
/// `direction(facing - 90°)`. Zero intent yields zero horizontal velocity.
fn movement_velocity(state: &PlayerState) -> Result<Vec3i, ZoneError> {
    const FORWARD: u16 = 0;
    const LEFT: u16 = YAW_QUARTER_TURN;
    const BACKWARD: u16 = 2 * YAW_QUARTER_TURN;
    const RIGHT: u16 = 3 * YAW_QUARTER_TURN;
    let offset = match (state.forward, state.strafe) {
        (Axis::Zero, Axis::Zero) => return Ok(Vec3i::ZERO),
        (Axis::Positive, Axis::Zero) => FORWARD,
        (Axis::Positive, Axis::Negative) => FORWARD + YAW_EIGHTH_TURN,
        (Axis::Zero, Axis::Negative) => LEFT,
        (Axis::Negative, Axis::Negative) => LEFT + YAW_EIGHTH_TURN,
        (Axis::Negative, Axis::Zero) => BACKWARD,
        (Axis::Negative, Axis::Positive) => BACKWARD + YAW_EIGHTH_TURN,
        (Axis::Zero, Axis::Positive) => RIGHT,
        (Axis::Positive, Axis::Positive) => RIGHT + YAW_EIGHTH_TURN,
    };
    let speed = match state.forward {
        Axis::Negative => BACKPEDAL_SPEED_SUBUNITS_PER_TICK,
        Axis::Zero | Axis::Positive => RUN_SPEED_SUBUNITS_PER_TICK,
    };
    let (x, z) = trig::direction(state.facing.wrapping_add(offset));
    let component = |unit| {
        trig::checked_scale(speed, unit).ok_or_else(|| ZoneError::new("movement velocity overflow"))
    };
    Ok(Vec3i::new(component(x)?, 0, component(z)?))
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
            (5, ZoneCommand::StartAttack),
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
    fn excess_intents_are_dropped_and_reported_without_a_session_error() {
        let mut zone = ZoneSimulation::new(ZoneId::new(1));
        zone.add_player(1).unwrap();
        for sequence in 1..=u32::try_from(MAX_PENDING_INTENTS).unwrap() {
            zone.apply_command(1, sequence, ZoneCommand::StopAttack)
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
            [crate::QueuedIntent::from(PlayerIntent::StopAttack); MAX_PENDING_INTENTS],
            "the excess intents are dropped"
        );
        assert!(player.combat.intents_dropped);
        assert_eq!(
            zone.apply_command(1, 101, ZoneCommand::StopAttack)
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
        zone.apply_command(1, 102, ZoneCommand::StopAttack).unwrap();
        zone.advance_tick().unwrap();
        assert!(zone.snapshot_for_player(1).unwrap().events.is_empty());
    }

    #[test]
    fn a_dropped_intent_survives_recovery_and_needs_a_full_queue() {
        let mut original = ZoneSimulation::new(ZoneId::new(1));
        original.add_player(1).unwrap();
        for sequence in 1..=u32::try_from(MAX_PENDING_INTENTS).unwrap() + 1 {
            original
                .apply_command(1, sequence, ZoneCommand::StopAttack)
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
