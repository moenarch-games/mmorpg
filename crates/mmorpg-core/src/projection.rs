//! Player-scoped projections: what one player may see of the zone.
//!
//! A projection carries the viewer's own exact state (including class,
//! resource, cast, cooldowns and auras), its target's target and the
//! target's cast and auras, this tick's events and the relevant units within the interest radius in
//! priority order: the viewer first, then the viewer's current target (alive
//! or corpse), then ascending `(squared XZ distance, kind, id)`, and breakable props
//! (#288) after every other unit in the same order, capped at
//! [`MAX_VISIBLE_ENTITIES`]. Canonical state never leaves the zone this way.

use std::collections::BTreeSet;

use crate::creature::Life;
use crate::entity::body_id;
use crate::snapshot::CreatureAi;
use crate::unit::health_percent;
use crate::zone::saturate_i8;
use crate::{
    CreatureBehaviour, EntityKind, EntityRef, INTEREST_RADIUS_UNITS, MAX_VISIBLE_ENTITIES,
    PLAYER_HALF_EXTENTS_UNITS, PlayerId, PlayerProjection, ZoneError, ZoneEvent, ZoneId,
    ZoneSimulation,
};

/// Presentation flags of a visible unit.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct EntityFlags {
    pub dead: bool,
    /// On a threat table, engaged, or recently attacking or attacked.
    pub in_combat: bool,
    /// Explicit weapon mode for projected player posture (#802), not the combat timer.
    pub weapon_drawn: bool,
    /// A player's melee weapon family (#914): its main-hand weapon's, else its class's own;
    /// `None` for every other unit. Presentation only: the zone resolves every strike.
    pub weapon: Option<crate::WeaponFamily>,
    /// A player's named main-hand weapon (#918), drawn with its own model; `None` otherwise.
    pub named_weapon: Option<crate::NamedWeapon>,
    /// A player's two-handed main-hand weapon (#1073): which great weapon's model and strike
    /// clip clients show; `None` otherwise. Presentation only: the family row resolves it.
    pub great_weapon: Option<crate::GreatWeapon>,
    /// A player in wolf form (#957), whom clients draw as the wolf. Presentation only.
    pub shapeshifted: bool,
    /// The load a player carries in both arms (#1077), which clients show with the loaded walk
    /// and run; `None` otherwise.
    pub load: Option<crate::LoadKind>,
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
    /// A player climbing or mantling (#490), whose facing then faces the wall, or rolling (#516).
    pub traversal: crate::TraversalView,
    /// A sitting player or NPC, or a resident in bed or getting up (#619).
    pub posture: crate::PostureView,
}

/// One unit in a player-visible projection.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EntitySnapshot {
    pub kind: EntityKind,
    pub id: u32,
    /// Creature template ID, NPC ID, or for players 0 without a class and
    /// otherwise `1 + class × 2 + sex`.
    pub appearance: u32,
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

/// The viewer's shield guard (#156).
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct GuardView {
    /// The lane the guard key holds.
    pub raised: Option<crate::StrikeHeight>,
    /// The zone blocks blows in `raised` right now: a ready, grounded guard.
    pub protecting: bool,
    /// Ticks of guard-break recovery left.
    pub broken: u16,
}

/// The viewer's own exact unit state.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ViewerState {
    pub copper: u32,
    pub experience: u32,
    pub experience_to_next_level: u32,
    pub health: u32,
    pub max_health: u32,
    pub level: u8,
    pub dead: bool,
    pub in_combat: bool,
    /// Explicit draw/sheathe state, independent of `in_combat`.
    pub weapon_drawn: bool,
    pub target: Option<EntityRef>,
    pub class: Option<crate::ClassChoice>,
    /// Present exactly when the viewer has a class.
    pub resource: Option<ResourceView>,
    pub cast: Option<CastView>,
    pub global_cooldown: u16,
    /// The length the running global cooldown began with, haste included (#884, #885).
    pub global_cooldown_total: u16,
    /// Inclusive melee damage range, the equipment bonus included.
    pub damage: [u16; 2],
    pub guard: GuardView,
    /// A living Warden's melee stance toward its target (#155).
    pub stance: Option<crate::MeleeStance>,
    /// A pending sword → Shield Bash follow-up (#157).
    pub combo: Option<crate::ComboPhase>,
    /// Ticks of breath left, [`crate::water::BREATH_MAX_TICKS`] when full (#274).
    pub breath: u16,
    /// Waystones the viewer unlocked (#145).
    pub waystones: crate::WaystoneSet,
    /// Named areas the viewer discovered (#916), for the exploration map (#983).
    pub discoveries: std::collections::BTreeSet<crate::AreaId>,
    /// The harm a Divine Reckoning would draw on against the viewer's target (#936): 0 without
    /// a creature target or stored harm.
    pub reckoning: u16,
    /// The viewer's sword mastery (#915): level and practice toward the next.
    pub mastery: crate::Mastery,
}

/// What a sheet's content was built from (#1056): two equal stamps mean equal sheet bytes.
/// Built from revision stamps, which are unique per value, so a stamp
/// never repeats for different content, whichever character or zone it came from.
pub type SheetStamp = [u64; 4];

/// The stamps of the sheets a publisher may hold on to between ticks (#1056): the bag, the
/// quest sheet, the world sheet and the waystone sheet. `None` marks a sheet built every
/// time (the world sheet while the raid signal shows). The loot, ability, party and boss
/// loot sheets follow per-tick combat state and are always built.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SheetStamps {
    /// Bag, equipment and inventory revision.
    pub bag: Option<SheetStamp>,
    /// Quest log, the bag it counts and the depths record, and whether it is night.
    pub quests: Option<SheetStamp>,
    /// The zone-wide doors, pieces and spent veins of [`crate::ZoneSimulation`]'s tick.
    pub world: Option<SheetStamp>,
    /// Unlocked waystones and discovered areas.
    pub waystones: Option<SheetStamp>,
}

/// Deterministic work one projection did beyond its interest query (#1056).
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ProjectionWork {
    /// Entity records built: the relevant units within the priority cap, never every
    /// candidate.
    pub entity_records_built: usize,
    /// Stamped sheets (bag, quests, world, waystones) built because the held stamp differed.
    pub sheets_built: usize,
}

/// A projection addressed to `viewer_id`. Entities are in priority order: the
/// viewer first, its target next when visible, then ascending
/// `(squared XZ distance, kind, id)`, at most [`MAX_VISIBLE_ENTITIES`] records.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ZoneSnapshot {
    pub content_revision: u64,
    pub acknowledged_sequence: u32,
    pub viewer_id: PlayerId,
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
    /// The bag's revision, travelling with the bag sheet; 0 before a client received one.
    pub inventory_revision: u64,
    /// The complete self bag. A zone always projects it; a client lacks it only until its
    /// first bag sheet arrives (the protocol sends sheets on change, #544).
    pub inventory: Option<crate::Inventory>,
    /// Equipment, present exactly with the bag under the same revision; its
    /// stat totals are [`crate::Equipment::totals`].
    pub equipment: Option<crate::Equipment>,
    /// Quest log, turned-in quests and NPC markers; like the bag, absent only on a client
    /// that has not received its sheet yet.
    pub quests: Option<crate::QuestSheet>,
    /// Complete eligible selected corpse sheet.
    pub loot: Option<crate::LootView>,
    /// Feedback the viewer received this tick.
    pub events: Vec<ZoneEvent>,
    /// Chat lines the viewer heard this tick, in delivery order.
    pub chat: Vec<crate::ChatLine>,
    pub entities: Vec<EntitySnapshot>,
    /// Every content door's state, in door-ID order (#158).
    pub doors: Vec<crate::DoorView>,
    /// Projectiles within the interest radius, nearest first (#286).
    pub projectiles: Vec<crate::ProjectileView>,
    /// Pieces of broken props within the interest radius, nearest first (#288).
    pub debris: Vec<crate::DebrisView>,
    /// Loads set down within the interest radius (#1077), nearest first, at most
    /// [`crate::MAX_VISIBLE_LOADS`].
    pub loads: Vec<crate::LoadView>,
    /// Destructible pieces that are not intact, in ID order (#305).
    pub pieces: Vec<crate::PieceView>,
    /// The content ore veins spent at this tick, in content order (#917).
    pub depleted_veins: Vec<u32>,
    /// Every content seal's glyphs and opening, in content order (#1019).
    pub seals: Vec<crate::SealView>,
    /// Every content ward's exposure, in content order (#1019).
    pub wards: Vec<crate::WardView>,
    /// Summoned terrain near the viewer, nearest first (#293).
    pub summoned: Vec<crate::SummonedView>,
    /// Fired pressure traps near the viewer, nearest first (#300); blades and spikes come from
    /// content and the tick alone.
    pub traps: Vec<crate::TrapView>,
    /// The night raid's signals while the viewer is near its target (#530).
    pub raid: Option<crate::RaidView>,
    /// The viewer's copy of the boss loot pool, when it is eligible for this kill (#404).
    pub depths_loot: Option<crate::DepthsLootView>,
    /// The viewer's party standing, once the directory has sent one (#404).
    pub party: Option<crate::PartyState>,
}

impl ZoneSimulation {
    pub fn snapshot_for_player(&self, player_id: PlayerId) -> Result<ZoneSnapshot, ZoneError> {
        Ok(self.project_for_player(player_id)?.snapshot)
    }

    /// Queries the same authoritative projection as `snapshot_for_player`, with
    /// operation counts for workload analysis. Does not mutate canonical state.
    pub fn project_for_player(&self, player_id: PlayerId) -> Result<PlayerProjection, ZoneError> {
        self.project_for_publish(player_id, &SheetStamps::default())
    }

    /// The projection a publisher sends (#1056): [`Self::project_for_player`], except that a
    /// sheet whose current stamp equals the one in `held` is left out (an empty bag, quest,
    /// world or waystone field) because the publisher still holds its encoding. The returned
    /// [`PlayerProjection::sheets`] names what each sheet was built from, and
    /// [`PlayerProjection::held`] echoes `held`, so the encoder can tell a left-out sheet from
    /// an empty one. Does not mutate canonical state.
    pub fn project_for_publish(
        &self,
        player_id: PlayerId,
        held: &SheetStamps,
    ) -> Result<PlayerProjection, ZoneError> {
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
        // The cheap position test runs first; records are built only for the units that
        // survive the radius and the priority cap (#1056).
        let mut relevant = Vec::with_capacity(candidates.len());
        for candidate in candidates {
            let Some([x, z]) = self.entity_xz(candidate)? else {
                continue;
            };
            let dx = i128::from(x) - i128::from(center.x);
            let dz = i128::from(z) - i128::from(center.z);
            let distance_squared = (dx * dx) + (dz * dz);
            if distance_squared <= radius_squared {
                let class = if candidate == self_entity {
                    0
                } else if viewer.target == Some(candidate) {
                    1
                } else if self.is_prop(candidate) {
                    // Props (#288) yield every slot to players, creatures and NPCs.
                    3
                } else {
                    2
                };
                relevant.push((class, distance_squared, candidate));
            }
        }
        stats.relevant = relevant.len();
        // Relevance policy: the viewer, its target, then the nearest units.
        // Kind and ID are unique together, so this is a total order.
        relevant.sort_unstable();
        relevant.truncate(MAX_VISIBLE_ENTITIES);
        let mut entities = Vec::with_capacity(relevant.len());
        for (_, _, candidate) in relevant {
            if let Some(entity) = self.entity_snapshot(candidate, player_id, &threatened)? {
                entities.push(entity);
            }
        }
        let mut work = ProjectionWork {
            entity_records_built: entities.len(),
            sheets_built: 0,
        };

        // Sheets (#1056): each is built only when its stamp differs from the held one.
        let (views, views_stamp) = self.zone_views();
        let raid = self.raid_view(player_id, [center.x, center.z]);
        let night = crate::is_night(crate::time_of_day(self.tick));
        let sheets = SheetStamps {
            bag: Some([
                viewer.inventory.revision(),
                viewer.equipment.revision(),
                viewer.inventory_revision,
                0,
            ]),
            quests: Some([
                viewer.quests.revision(),
                viewer.inventory.revision(),
                viewer.depths.revision(),
                u64::from(night),
            ]),
            // The raid signal is per viewer and rare; a world sheet carrying it is always built.
            world: raid.is_none().then_some([views_stamp, 0, 0, 0]),
            waystones: Some([
                viewer.waystones.revision(),
                viewer.discoveries.revision(),
                0,
                0,
            ]),
        };
        let mut build = |current: Option<SheetStamp>, held: Option<SheetStamp>| {
            let build = current.is_none() || current != held;
            work.sheets_built += usize::from(build);
            build
        };
        let (build_bag, build_quests, build_world, build_waystones) = (
            build(sheets.bag, held.bag),
            build(sheets.quests, held.quests),
            build(sheets.world, held.world),
            build(sheets.waystones, held.waystones),
        );

        // Events naming a unit the viewer cannot see stay hidden; most ticks carry none.
        let events = viewer
            .events
            .iter()
            .copied()
            .filter(|event| {
                event.observation_refs(player_id).is_none_or(|refs| {
                    refs.into_iter().flatten().all(|entity| {
                        entities
                            .iter()
                            .any(|projected| projected.entity() == entity)
                    })
                })
            })
            .collect();
        let max_health = viewer.max_health();
        Ok(PlayerProjection {
            snapshot: ZoneSnapshot {
                content_revision: self.content.revision(),
                acknowledged_sequence: viewer.last_sequence,
                viewer_id: player_id,
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
                    weapon_drawn: viewer.weapon_drawn,
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
                    global_cooldown_total: viewer.global_cooldown_total,
                    damage: viewer.melee_damage(),
                    guard: GuardView {
                        raised: viewer.guard.raised,
                        protecting: self.guard_lane(player_id)?.is_some(),
                        broken: viewer.guard.broken,
                    },
                    stance: self.melee_stance(player_id, viewer.target)?,
                    combo: self.combo_phase(viewer),
                    breath: viewer.breath.remaining,
                    waystones: if build_waystones {
                        (*viewer.waystones).clone()
                    } else {
                        crate::WaystoneSet::EMPTY
                    },
                    discoveries: if build_waystones {
                        (*viewer.discoveries).clone()
                    } else {
                        BTreeSet::new()
                    },
                    reckoning: self.harm_against(player_id, viewer.target),
                    mastery: viewer.mastery.sword,
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
                inventory: build_bag.then(|| (*viewer.inventory).clone()),
                equipment: build_bag.then_some(*viewer.equipment),
                quests: build_quests.then(|| {
                    crate::quest::quest_sheet(
                        &self.content,
                        &viewer.quests,
                        &viewer.inventory,
                        *viewer.depths,
                        night,
                    )
                }),
                loot: match viewer.target {
                    Some(EntityRef::Creature(id)) => self.loot_view_for(player_id, id)?,
                    _ => None,
                },
                events,
                chat: viewer.chat.clone(),
                entities,
                doors: if build_world {
                    views.doors.clone()
                } else {
                    Vec::new()
                },
                projectiles: self.projectile_views([center.x, center.y, center.z]),
                debris: self.debris_views([center.x, center.y, center.z]),
                loads: self.visible_loads(player_id, [center.x, center.z], radius_squared),
                pieces: if build_world {
                    views.pieces.clone()
                } else {
                    Vec::new()
                },
                depleted_veins: if build_world {
                    views.depleted_veins.clone()
                } else {
                    Vec::new()
                },
                seals: if build_world {
                    views.seals.clone()
                } else {
                    Vec::new()
                },
                wards: if build_world {
                    views.wards.clone()
                } else {
                    Vec::new()
                },
                summoned: self.summoned_views([center.x, center.y, center.z]),
                traps: self.trap_views([center.x, center.y, center.z]),
                raid,
                party: self.parties.get(&player_id).cloned(),
                depths_loot: self.depths_loot_view(player_id),
            },
            stats,
            work,
            sheets,
            held: *held,
        })
    }

    /// A unit's projected XZ position, or `None` when it is not visible: the same values
    /// [`Self::entity_snapshot`] projects, read without building the record (#1056).
    fn entity_xz(&self, entity: EntityRef) -> Result<Option<[i32; 2]>, ZoneError> {
        Ok(match entity {
            EntityRef::Player(player_id) => {
                if !self.players.contains_key(&player_id) {
                    return Err(ZoneError::new("unknown player"));
                }
                let position = self.player_position(player_id)?;
                Some([position.x, position.z])
            }
            EntityRef::Creature(creature_id) => {
                let creature = self
                    .creatures
                    .get(&creature_id)
                    .ok_or_else(|| ZoneError::new("unknown creature"))?;
                self.creature_position(creature_id, creature)?
                    .map(|[x, _, z]| [x, z])
            }
            EntityRef::Npc(npc_id) if self.npc_hidden(npc_id) => None,
            EntityRef::Npc(npc_id) => Some(
                self.npc_feet(npc_id)
                    .ok_or_else(|| ZoneError::new("unknown npc"))?,
            ),
            EntityRef::Minion(_) => {
                let position = self
                    .world
                    .body(body_id(entity))
                    .ok_or_else(|| ZoneError::new("minion physics body is missing"))?
                    .position();
                Some([position.x, position.z])
            }
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
            EntityRef::Npc(_) | EntityRef::Minion(_) => return TargetDetail::default(),
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
            EntityRef::Minion(minion_id) => self.minions.get(&minion_id)?.target,
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
                let (position, velocity) = (body.position(), body.velocity_units());
                EntitySnapshot {
                    kind: EntityKind::Player,
                    id: player_id,
                    appearance: player
                        .class
                        .map_or(0, |class| u32::from(class.appearance())),
                    position: [position.x, position.y, position.z],
                    velocity: [velocity.x, velocity.y, velocity.z].map(saturate_i8),
                    facing: player.traversal.wall().unwrap_or(player.facing),
                    level: player.level,
                    health_percent: health_percent(player.health, player.max_health()),
                    flags: EntityFlags {
                        traversal: crate::TraversalView::of(player.traversal, player.roll),
                        posture: if player.sitting.is_sitting() {
                            crate::PostureView::Sitting
                        } else {
                            crate::PostureView::Free
                        },
                        hostile: self.players_are_hostile(viewer, player_id),
                        attackable: self.players_are_hostile(viewer, player_id)
                            && player.is_alive(),
                        dead: !player.is_alive(),
                        in_combat: player.combat_timer > 0 || threatened.contains(&player_id),
                        weapon_drawn: player.weapon_drawn,
                        weapon: Some(crate::melee::weapon_family(player)),
                        named_weapon: player
                            .equipment
                            .item(crate::EquipmentSlot::MainHand)
                            .and_then(crate::item_find)
                            .and_then(|find| find.named),
                        great_weapon: player
                            .equipment
                            .item(crate::EquipmentSlot::MainHand)
                            .and_then(crate::GreatWeapon::of_item),
                        shapeshifted: crate::aura::has_kind(
                            &player.auras,
                            crate::AuraKind::Shapeshift,
                        ),
                        load: self.carried_load(player_id),
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
                        let (position, velocity) = (body.position(), body.velocity_units());
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
                        creature.max_health(&self.content, template),
                    ),
                    flags: EntityFlags {
                        dead: !alive,
                        weapon_drawn: false,
                        weapon: None,
                        named_weapon: None,
                        great_weapon: None,
                        shapeshifted: false,
                        load: None,
                        in_combat: alive
                            && (creature.ai == CreatureAi::Engaged || creature.combat_timer > 0),
                        hostile: template.behaviour == CreatureBehaviour::Aggressive,
                        attackable: alive,
                        tapped_by_other: creature.tapped_by.is_some_and(|tapper| {
                            tapper != viewer && !self.share_party(tapper, viewer)
                        }),
                        lootable: !alive
                            && (creature.tapped_by == Some(viewer)
                                || creature.loot_party.contains(&viewer))
                            && creature.loot.is_some_and(crate::corpse_loot::has_rewards),
                        evading: matches!(creature.ai, CreatureAi::Evading { .. }),
                        targets_viewer,
                        traversal: crate::TraversalView::Free,
                        posture: crate::PostureView::Free,
                    },
                }
            }
            // A confronted NPC (#1019) is out of the world while its foe stands, and a departed one
            // out of its viewer's.
            EntityRef::Npc(npc_id)
                if self.npc_hidden(npc_id) || self.npc_departed(viewer, npc_id) =>
            {
                return Ok(None);
            }
            EntityRef::Npc(npc_id) => {
                let npc = self
                    .content
                    .npc(npc_id)
                    .ok_or_else(|| ZoneError::new("unknown npc"))?;
                let placement = self
                    .npc_placement(npc_id)
                    .ok_or_else(|| ZoneError::new("unknown npc"))?;
                // A hunter on its outing (#991) shows how it went.
                let hunting = self.hunt_placement(npc_id).map(|(_, posture)| posture);
                // A merchant on its cart journey (#992) shows its cart, its fright or its wounds.
                let cart = self
                    .cart_placement(npc_id)
                    .map(|(_, posture, health)| (posture, health));
                EntitySnapshot {
                    kind: EntityKind::Npc,
                    id: npc_id.get(),
                    appearance: npc_id.get(),
                    position: [
                        placement.position[0],
                        self.content
                            .definition()
                            .ground_at(placement.position[0], placement.position[1])
                            + PLAYER_HALF_EXTENTS_UNITS[1],
                        placement.position[1],
                    ],
                    velocity: [placement.velocity[0], 0, placement.velocity[1]],
                    facing: placement.facing,
                    level: npc.level,
                    // A defence's civilian (#1019) shows what it has left, a fallen one barely
                    // alive; a guard injured in a lost raid (#531), or a civilian still lying
                    // wounded after a defence, shows its wounds.
                    health_percent: if let Some(health) = self.civilian_health(npc_id) {
                        health.max(1)
                    } else if let Some(health) = self.sheep_health(npc_id) {
                        // A sheep of a watch (#1019) shows what it has left, a fallen one barely alive.
                        health.max(1)
                    } else if self
                        .content
                        .raid()
                        .and_then(|site| site.injured_at(npc_id, &self.raid.aftermath))
                        .is_some()
                    {
                        crate::raid::INJURED_HEALTH_PERCENT
                    } else if let Some((_, health)) = cart {
                        health
                    } else if hunting == Some(crate::PostureView::Hurt) {
                        crate::hunt::INJURED_HEALTH_PERCENT
                    } else {
                        100
                    },
                    flags: EntityFlags {
                        posture: hunting
                            .or(cart.map(|(posture, _)| posture))
                            .unwrap_or_else(|| self.npc_posture(npc_id)),
                        ..EntityFlags::default()
                    },
                }
            }
            // A summoned minion (#295): its summoning ability is its appearance. Minions fight
            // creatures only, so no player may attack one.
            EntityRef::Minion(minion_id) => {
                let minion = self
                    .minions
                    .get(&minion_id)
                    .ok_or_else(|| ZoneError::new("unknown minion"))?;
                let body = self
                    .world
                    .body(body_id(entity))
                    .ok_or_else(|| ZoneError::new("minion physics body is missing"))?;
                let (position, velocity) = (body.position(), body.velocity_units());
                EntitySnapshot {
                    kind: EntityKind::Minion,
                    id: minion_id.get(),
                    appearance: minion.ability.get(),
                    position: [position.x, position.y, position.z],
                    velocity: [velocity.x, velocity.y, velocity.z].map(saturate_i8),
                    facing: minion.facing,
                    level: minion.level,
                    health_percent: health_percent(minion.health, minion.max_health()),
                    flags: EntityFlags {
                        in_combat: minion.target.is_some(),
                        ..EntityFlags::default()
                    },
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
