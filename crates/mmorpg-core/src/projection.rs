//! Player-scoped projections: what one player may see of the zone.
//!
//! A projection carries the viewer's own exact state (including class,
//! resource, cast, cooldowns and auras), its target's target and the
//! target's cast and auras, this tick's events and the relevant units within the interest radius in
//! priority order: the viewer first, then the viewer's current target (alive
//! or corpse), then ascending `(squared XZ distance, kind, id)`, capped at
//! [`MAX_VISIBLE_ENTITIES`]. Canonical state never leaves the zone this way.

use std::collections::BTreeSet;

use crate::creature::Life;
use crate::entity::body_id;
use crate::snapshot::CreatureAi;
use crate::unit::health_percent;
use crate::zone::saturate_i8;
use crate::{
    CreatureBehaviour, EntityKind, EntityRef, INTEREST_RADIUS_UNITS, MAX_VISIBLE_ENTITIES,
    PLAYER_HALF_EXTENTS_UNITS, PlayerId, PlayerProjection, SNAPSHOT_SCHEMA_VERSION, ZoneError,
    ZoneEvent, ZoneId, ZoneSimulation,
};

/// Presentation flags of a visible unit.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct EntityFlags {
    pub dead: bool,
    /// On a threat table, engaged, or recently attacking or attacked.
    pub in_combat: bool,
    /// An aggressive creature: it attacks players on sight.
    pub hostile: bool,
    /// The viewer may attack it (living creatures).
    pub attackable: bool,
    /// A creature tapped by a player other than the viewer.
    pub tapped_by_other: bool,
    pub evading: bool,
    /// Its own target is the viewer.
    pub targets_viewer: bool,
    /// Remaining rewards on a corpse owned by this viewer. Range is checked on claim.
    pub lootable: bool,
}

/// One unit in a player-visible projection.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EntitySnapshot {
    pub kind: EntityKind,
    pub id: u32,
    /// Creature template ID, NPC ID, or for players 0 without a class and
    /// otherwise `1 + class × 2 + sex`.
    pub appearance: u16,
    pub position: [i32; 3],
    /// Presentation-only velocity, saturated to the `i8` range per axis.
    /// Canonical state keeps the exact physics velocity.
    pub velocity: [i8; 3],
    pub facing: u16,
    pub level: u8,
    /// Rounded up, so living units never show 0.
    pub health_percent: u8,
    pub flags: EntityFlags,
}

impl EntitySnapshot {
    #[must_use]
    pub const fn entity(&self) -> EntityRef {
        EntityRef::new(self.kind, self.id)
    }
}

/// A class resource as the viewer sees it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ResourceView {
    pub kind: crate::ResourceKind,
    pub value: u16,
    pub max: u16,
}

/// A cast or channel in progress: `elapsed` of `total` ticks.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CastView {
    pub ability: crate::AbilityId,
    pub elapsed: u16,
    pub total: u16,
    pub channel: bool,
}

/// An aura as players see it; see [`crate::Aura`] for `amount`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AuraView {
    pub ability: crate::AbilityId,
    pub kind: crate::AuraKind,
    pub remaining: u16,
    pub amount: u16,
}

/// The viewer's target's cast and auras: empty without a visible living
/// player or creature target.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct TargetDetail {
    pub cast: Option<CastView>,
    pub auras: Vec<AuraView>,
}

/// The viewer's own exact unit state.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ViewerState {
    pub copper: u32,
    pub experience: u32,
    pub experience_to_next_level: u32,
    pub health: u32,
    pub max_health: u32,
    pub level: u8,
    pub dead: bool,
    pub in_combat: bool,
    pub auto_attacking: bool,
    pub target: Option<EntityRef>,
    pub class: Option<crate::ClassChoice>,
    /// Present exactly when the viewer has a class.
    pub resource: Option<ResourceView>,
    pub cast: Option<CastView>,
    pub global_cooldown: u16,
    /// Inclusive melee damage range, the equipment bonus included.
    pub damage: [u16; 2],
}

/// A projection addressed to `viewer_id`. Entities are in priority order: the
/// viewer first, its target next when visible, then ascending
/// `(squared XZ distance, kind, id)`, at most [`MAX_VISIBLE_ENTITIES`] records.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ZoneSnapshot {
    pub content_revision: u64,
    pub acknowledged_sequence: u32,
    pub viewer_id: PlayerId,
    pub schema_version: u16,
    pub zone_id: ZoneId,
    pub tick: u64,
    pub viewer: ViewerState,
    /// The viewer's running cooldowns in ability order.
    pub cooldowns: Vec<crate::Cooldown>,
    /// The viewer's auras in slot order.
    pub auras: Vec<AuraView>,
    /// The viewer's target's own target, if any.
    pub target_of_target: Option<EntityRef>,
    /// The viewer's target's cast and auras.
    pub target_detail: TargetDetail,
    /// Always repeated; an absent sheet must not be mistaken for an empty bag.
    pub inventory_revision: u64,
    /// Complete self bag on admission/change ticks and every ten ticks.
    pub inventory: Option<crate::Inventory>,
    /// Equipment, sent exactly with the bag under the same revision; its
    /// stat totals are [`crate::Equipment::totals`].
    pub equipment: Option<crate::Equipment>,
    /// Quest log, turned-in quests and NPC markers, sent exactly with the
    /// bag sheet.
    pub quests: Option<crate::QuestSheet>,
    /// Complete eligible selected corpse sheet, repeated every projection.
    pub loot: Option<crate::LootView>,
    /// Feedback the viewer received this tick.
    pub events: Vec<ZoneEvent>,
    /// Chat lines the viewer heard this tick, in delivery order.
    pub chat: Vec<crate::ChatLine>,
    pub entities: Vec<EntitySnapshot>,
}

impl ZoneSimulation {
    pub fn snapshot_for_player(&self, player_id: PlayerId) -> Result<ZoneSnapshot, ZoneError> {
        Ok(self.project_for_player(player_id)?.snapshot)
    }

    /// Queries the same authoritative projection as `snapshot_for_player`, with
    /// operation counts for workload analysis. Does not mutate canonical state.
    pub fn project_for_player(&self, player_id: PlayerId) -> Result<PlayerProjection, ZoneError> {
        let viewer = self
            .players
            .get(&player_id)
            .ok_or_else(|| ZoneError::new("unknown player"))?;
        let center = self.player_position(player_id)?;
        let threatened = self.threatened_players();

        let radius = i128::from(INTEREST_RADIUS_UNITS);
        let radius_squared = radius * radius;
        let (candidates, mut stats) = self.interest.candidates(center.x, center.z);
        let self_entity = EntityRef::Player(player_id);
        let mut relevant = Vec::new();
        for candidate in candidates {
            let Some(entity) = self.entity_snapshot(candidate, player_id, &threatened)? else {
                continue;
            };
            let dx = i128::from(entity.position[0]) - i128::from(center.x);
            let dz = i128::from(entity.position[2]) - i128::from(center.z);
            let distance_squared = (dx * dx) + (dz * dz);
            if distance_squared <= radius_squared {
                let class = if candidate == self_entity {
                    0
                } else if viewer.target == Some(candidate) {
                    1
                } else {
                    2
                };
                relevant.push((class, distance_squared, candidate, entity));
            }
        }
        stats.relevant = relevant.len();
        // Relevance policy: the viewer, its target, then the nearest units.
        // Kind and ID are unique together, so this is a total order.
        relevant.sort_unstable_by(|left, right| {
            (left.0, left.1, left.2).cmp(&(right.0, right.1, right.2))
        });
        relevant.truncate(MAX_VISIBLE_ENTITIES);
        let entities = relevant
            .into_iter()
            .map(|(_, _, _, entity)| entity)
            .collect();

        let max_health = viewer.max_health();
        let sheet = self.tick == viewer.inventory_changed_at
            || self.tick == viewer.quests_changed_at
            || self.tick.is_multiple_of(crate::INVENTORY_RESEND_TICKS);
        Ok(PlayerProjection {
            snapshot: ZoneSnapshot {
                content_revision: self.content.revision(),
                acknowledged_sequence: viewer.last_sequence,
                viewer_id: player_id,
                schema_version: SNAPSHOT_SCHEMA_VERSION,
                zone_id: self.zone_id,
                tick: self.tick,
                viewer: ViewerState {
                    copper: viewer.copper,
                    experience: viewer.experience,
                    experience_to_next_level: crate::experience_to_next_level(viewer.level)
                        .unwrap_or(0),
                    health: viewer.health,
                    max_health,
                    level: viewer.level,
                    dead: !viewer.is_alive(),
                    in_combat: viewer.combat_timer > 0 || threatened.contains(&player_id),
                    auto_attacking: viewer.auto_attack,
                    target: viewer.target,
                    class: viewer.class,
                    resource: viewer.class.map(|choice| {
                        let kind = choice.class.resource();
                        ResourceView {
                            kind,
                            value: viewer.resource.value,
                            max: kind.max(viewer.level),
                        }
                    }),
                    cast: viewer.cast.and_then(cast_view),
                    global_cooldown: viewer.global_cooldown,
                    damage: viewer.melee_damage(),
                },
                cooldowns: viewer.cooldowns.clone(),
                auras: aura_views(&viewer.auras),
                target_of_target: viewer.target.and_then(|target| self.target_of(target)),
                target_detail: match viewer.target {
                    Some(target) if self.visible_to(player_id, target)? => {
                        self.target_detail(target)
                    }
                    _ => TargetDetail::default(),
                },
                inventory_revision: viewer.inventory_revision,
                inventory: sheet.then(|| viewer.inventory.clone()),
                equipment: sheet.then_some(viewer.equipment),
                quests: sheet.then(|| {
                    crate::quest::quest_sheet(&self.content, &viewer.quests, &viewer.inventory)
                }),
                loot: match viewer.target {
                    Some(EntityRef::Creature(id)) => self.loot_view_for(player_id, id)?,
                    _ => None,
                },
                events: viewer.events.clone(),
                chat: viewer.chat.clone(),
                entities,
            },
            stats,
        })
    }

    /// A living player's or creature's cast and auras.
    fn target_detail(&self, target: EntityRef) -> TargetDetail {
        let (cast, auras) = match target {
            EntityRef::Player(player_id) => match self.players.get(&player_id) {
                Some(player) if player.is_alive() => (player.cast, player.auras.as_slice()),
                _ => return TargetDetail::default(),
            },
            EntityRef::Creature(creature_id) => match self.creatures.get(&creature_id) {
                Some(creature) if creature.is_alive() => (creature.cast, creature.auras.as_slice()),
                _ => return TargetDetail::default(),
            },
            EntityRef::Npc(_) => return TargetDetail::default(),
        };
        TargetDetail {
            cast: cast.and_then(cast_view),
            auras: aura_views(auras),
        }
    }

    /// Whom a unit targets: a player's selection, an engaged creature's
    /// threat leader, and nothing for NPCs.
    fn target_of(&self, entity: EntityRef) -> Option<EntityRef> {
        match entity {
            EntityRef::Player(player_id) => self.players.get(&player_id)?.target,
            EntityRef::Creature(creature_id) => {
                let creature = self.creatures.get(&creature_id)?;
                (creature.is_alive() && creature.ai == CreatureAi::Engaged)
                    .then(|| creature.top_threat())
                    .flatten()
            }
            EntityRef::Npc(_) => None,
        }
    }

    /// The projected record of a unit, or `None` when it is not visible.
    fn entity_snapshot(
        &self,
        entity: EntityRef,
        viewer: PlayerId,
        threatened: &BTreeSet<PlayerId>,
    ) -> Result<Option<EntitySnapshot>, ZoneError> {
        let targets_viewer = self.target_of(entity) == Some(EntityRef::Player(viewer))
            && entity != EntityRef::Player(viewer);
        Ok(Some(match entity {
            EntityRef::Player(player_id) => {
                let player = self
                    .players
                    .get(&player_id)
                    .ok_or_else(|| ZoneError::new("unknown player"))?;
                let body = self
                    .world
                    .body(body_id(entity))
                    .ok_or_else(|| ZoneError::new("player physics body is missing"))?;
                let (position, velocity) = (body.position(), body.velocity());
                EntitySnapshot {
                    kind: EntityKind::Player,
                    id: player_id,
                    appearance: player.class.map_or(0, crate::ClassChoice::appearance),
                    position: [position.x, position.y, position.z],
                    velocity: [velocity.x, velocity.y, velocity.z].map(saturate_i8),
                    facing: player.facing,
                    level: player.level,
                    health_percent: health_percent(player.health, player.max_health()),
                    flags: EntityFlags {
                        dead: !player.is_alive(),
                        in_combat: player.combat_timer > 0 || threatened.contains(&player_id),
                        targets_viewer,
                        ..EntityFlags::default()
                    },
                }
            }
            EntityRef::Creature(creature_id) => {
                let creature = self
                    .creatures
                    .get(&creature_id)
                    .ok_or_else(|| ZoneError::new("unknown creature"))?;
                let (_, template) = Self::creature_content(&self.content, creature_id)?;
                let (position, velocity) = match creature.life {
                    Life::Alive => {
                        let body = self
                            .world
                            .body(body_id(entity))
                            .ok_or_else(|| ZoneError::new("creature physics body is missing"))?;
                        let (position, velocity) = (body.position(), body.velocity());
                        (
                            [position.x, position.y, position.z],
                            [velocity.x, velocity.y, velocity.z].map(saturate_i8),
                        )
                    }
                    Life::Corpse { position, .. } => (position, [0; 3]),
                    Life::Despawned { .. } => return Ok(None),
                };
                let alive = creature.is_alive();
                EntitySnapshot {
                    kind: EntityKind::Creature,
                    id: creature_id.get(),
                    appearance: template.id.get(),
                    position,
                    velocity,
                    facing: creature.facing,
                    level: creature.level,
                    health_percent: health_percent(
                        creature.health,
                        template.max_health(creature.level),
                    ),
                    flags: EntityFlags {
                        dead: !alive,
                        in_combat: alive
                            && (creature.ai == CreatureAi::Engaged || creature.combat_timer > 0),
                        hostile: template.behaviour == CreatureBehaviour::Aggressive,
                        attackable: alive,
                        tapped_by_other: creature.tapped_by.is_some_and(|tapper| tapper != viewer),
                        lootable: !alive
                            && creature.tapped_by == Some(viewer)
                            && creature.loot.is_some_and(crate::corpse_loot::has_rewards),
                        evading: matches!(creature.ai, CreatureAi::Evading { .. }),
                        targets_viewer,
                    },
                }
            }
            EntityRef::Npc(npc_id) => {
                let npc = self
                    .content
                    .npc(npc_id)
                    .ok_or_else(|| ZoneError::new("unknown npc"))?;
                EntitySnapshot {
                    kind: EntityKind::Npc,
                    id: npc_id.get(),
                    appearance: u16::try_from(npc_id.get()).unwrap_or(u16::MAX),
                    position: [
                        npc.position[0],
                        PLAYER_HALF_EXTENTS_UNITS[1],
                        npc.position[1],
                    ],
                    velocity: [0; 3],
                    facing: npc.facing,
                    level: npc.level,
                    health_percent: 100,
                    flags: EntityFlags::default(),
                }
            }
        }))
    }
}

fn cast_view(cast: crate::CastState) -> Option<CastView> {
    let ability = crate::ability_by_id(cast.ability)?;
    Some(CastView {
        ability: cast.ability,
        elapsed: cast.elapsed,
        total: ability.cast.ticks(),
        channel: ability.cast.is_channel(),
    })
}

fn aura_views(auras: &[crate::Aura]) -> Vec<AuraView> {
    auras
        .iter()
        .filter_map(|aura| {
            Some(AuraView {
                ability: aura.ability,
                kind: aura.kind()?,
                remaining: aura.remaining,
                amount: aura.amount,
            })
        })
        .collect()
}
