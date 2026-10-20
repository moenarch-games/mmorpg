//! The complete immutable content of one zone and its identity.
//!
//! A zone's content is its physical definition plus its named areas, creature
//! templates, creature spawns, NPCs, graveyard and optional loot rules. [`ZoneContent::new`]
//! validates the tables once and computes a fingerprint over a canonical byte
//! encoding of all of them. Canonical snapshots record the revision and
//! fingerprint instead of embedding the content, and recovery refuses content
//! whose identity differs (the content-addressed checkpoint rule).

use super::units::{
    CreatureSpawn, CreatureTemplate, MAX_CREATURE_HALF_EXTENT_UNITS, MAX_CREATURE_SPAWNS,
    MAX_CREATURE_TEMPLATES, MAX_NPCS, MAX_UNIT_NAME_BYTES, MAX_WANDER_RADIUS_UNITS, Npc,
};
use super::{MAX_CONTENT_COORDINATE_UNITS, ZoneDefinition};
use crate::ability::{AbilityId, AbilityUser, ability_by_id};
use crate::unit::{CORPSE_TICKS, MAX_SWING_DAMAGE, MAX_UNIT_LEVEL};
use crate::{
    CreatureId, CreatureTemplateId, LootOutcome, LootTable, MAX_PLAYERS_PER_ZONE, NpcId, NpcRole,
    PLAYER_HALF_EXTENTS_UNITS, VendorStock, ZoneAreas, ZoneError,
};

/// Player-origin combat eligibility for an explicitly admitted zone instance.
/// Shared-world zones are safe by default. Dedicated arena admission belongs to its host.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum PlayerCombatPolicy {
    #[default]
    Safe,
    /// Every distinct admitted player may fight every other admitted player.
    /// This does not implement in-world duels or their nonlethal/isolation rules.
    FreeForAll,
}

/// Immutable, validated zone content, shared between zones through `Arc`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ZoneContent {
    definition: ZoneDefinition,
    areas: ZoneAreas,
    templates: Vec<CreatureTemplate>,
    spawns: Vec<CreatureSpawn>,
    npcs: Vec<Npc>,
    graveyard: [i32; 2],
    fingerprint: u64,
    player_combat: PlayerCombatPolicy,
    rng_seed: u64,
    loot_revision: u64,
    loot_tables: Vec<(CreatureTemplateId, LootTable)>,
    ability_revision: u64,
    creature_abilities: Vec<(CreatureTemplateId, AbilityId)>,
    vendor_revision: u64,
    vendors: Vec<(NpcId, VendorStock)>,
    quest_revision: u64,
    quests: Vec<crate::Quest>,
    doors: Vec<crate::Door>,
    platforms: Vec<crate::Platform>,
    ramps: Vec<crate::Ramp>,
    sites: Vec<crate::StructureSite>,
    pieces: Vec<crate::StructurePiece>,
    routines: Vec<crate::NpcRoutine>,
    closed_at_night: Vec<NpcId>,
}

/// A body as `(centre, half extents)` in units.
type BodyBox = ([i32; 3], [i32; 3]);

impl ZoneContent {
    /// Validates and orders the unit tables:
    ///
    /// - IDs are unique per table; tables are bounded and sorted by ID;
    /// - templates have names, level ranges within `1..=MAX_UNIT_LEVEL`,
    ///   positive health and damage whose maximum-level values fit their
    ///   types, a positive swing time, a respawn time of at least the corpse
    ///   duration, and positive bounded half extents;
    /// - spawns name a known template and a wander radius in
    ///   `0..=MAX_WANDER_RADIUS_UNITS`; NPCs have names and levels;
    /// - every creature, NPC and graveyard body stands on y = 0 inside the
    ///   content coordinate range without overlapping a collider, another
    ///   unit's body or a player spawn slot (the graveyard may share room with
    ///   spawn slots and other released spirits).
    pub fn new(
        definition: ZoneDefinition,
        areas: ZoneAreas,
        mut templates: Vec<CreatureTemplate>,
        mut spawns: Vec<CreatureSpawn>,
        mut npcs: Vec<Npc>,
        graveyard: [i32; 2],
    ) -> Result<Self, ZoneError> {
        if templates.len() > MAX_CREATURE_TEMPLATES
            || spawns.len() > MAX_CREATURE_SPAWNS
            || npcs.len() > MAX_NPCS
        {
            return Err(ZoneError::new("zone unit content capacity reached"));
        }
        templates.sort_by_key(|template| template.id);
        spawns.sort_by_key(|spawn| spawn.id);
        npcs.sort_by_key(|npc| npc.id);
        if templates.windows(2).any(|pair| pair[0].id == pair[1].id)
            || spawns.windows(2).any(|pair| pair[0].id == pair[1].id)
            || npcs.windows(2).any(|pair| pair[0].id == pair[1].id)
        {
            return Err(ZoneError::new("duplicate unit content id"));
        }
        for template in &templates {
            validate_template(template)?;
        }

        let fixed_boxes = spawn_slot_boxes(&definition)?;
        let mut unit_boxes: Vec<BodyBox> = Vec::new();
        for spawn in &spawns {
            let template = templates
                .binary_search_by_key(&spawn.template, |template| template.id)
                .map(|index| &templates[index])
                .map_err(|_| ZoneError::new("creature spawn names an unknown template"))?;
            if !(0..=MAX_WANDER_RADIUS_UNITS).contains(&spawn.wander_radius)
                || (template.prop().is_some() && spawn.wander_radius != 0)
            {
                return Err(ZoneError::new("creature wander radius is out of range"));
            }
            let body = standing_body(spawn.position, template.body_half_extents(spawn.facing));
            validate_unit_body(&definition, body, &unit_boxes, &fixed_boxes)?;
            unit_boxes.push(body);
        }
        for npc in &npcs {
            validate_name(&npc.name)?;
            // Projections carry an NPC's ID as its `u16` appearance.
            if u16::try_from(npc.id.get()).is_err() {
                return Err(ZoneError::new("npc id must fit u16"));
            }
            if !(1..=MAX_UNIT_LEVEL).contains(&npc.level) {
                return Err(ZoneError::new("npc level is out of range"));
            }
            let body = standing_body(npc.position, PLAYER_HALF_EXTENTS_UNITS);
            validate_unit_body(&definition, body, &unit_boxes, &fixed_boxes)?;
            unit_boxes.push(body);
        }
        // Released spirits may stand where players spawn or where other
        // spirits stand, but never inside geometry or an NPC.
        let graveyard_body = standing_body(graveyard, PLAYER_HALF_EXTENTS_UNITS);
        let npc_boxes = &unit_boxes[spawns.len()..];
        validate_unit_body(&definition, graveyard_body, npc_boxes, &[])?;

        let mut content = Self {
            definition,
            areas,
            templates,
            spawns,
            npcs,
            graveyard,
            fingerprint: 0,
            player_combat: PlayerCombatPolicy::Safe,
            rng_seed: 0,
            loot_revision: 0,
            loot_tables: Vec::new(),
            ability_revision: 0,
            creature_abilities: Vec::new(),
            vendor_revision: 0,
            vendors: Vec::new(),
            quest_revision: 0,
            quests: Vec::new(),
            doors: Vec::new(),
            platforms: Vec::new(),
            ramps: Vec::new(),
            sites: Vec::new(),
            pieces: Vec::new(),
            routines: Vec::new(),
            closed_at_night: Vec::new(),
        };
        content.rng_seed = content.compute_simulation_fingerprint();
        content.fingerprint = content.compute_fingerprint();
        Ok(content)
    }

    /// Physical content only: no areas, creatures or NPCs, and the graveyard
    /// at spawn slot 0, which the definition already proves clear.
    #[must_use]
    pub fn from_definition(definition: ZoneDefinition) -> Self {
        let feet = definition.spawn_grid().feet(0).unwrap_or([0; 3]);
        let mut content = Self {
            definition,
            areas: ZoneAreas::default(),
            templates: Vec::new(),
            spawns: Vec::new(),
            npcs: Vec::new(),
            graveyard: [feet[0], feet[2]],
            fingerprint: 0,
            player_combat: PlayerCombatPolicy::Safe,
            rng_seed: 0,
            loot_revision: 0,
            loot_tables: Vec::new(),
            ability_revision: 0,
            creature_abilities: Vec::new(),
            vendor_revision: 0,
            vendors: Vec::new(),
            quest_revision: 0,
            quests: Vec::new(),
            doors: Vec::new(),
            platforms: Vec::new(),
            ramps: Vec::new(),
            sites: Vec::new(),
            pieces: Vec::new(),
            routines: Vec::new(),
            closed_at_night: Vec::new(),
        };
        content.rng_seed = content.compute_simulation_fingerprint();
        content.fingerprint = content.compute_fingerprint();
        content
    }

    /// Explicitly opts an isolated zone instance into player combat. The content fingerprint
    /// binds recovery to this policy, so restoring safe content cannot enable arena attacks.
    #[must_use]
    pub fn with_player_combat(mut self, policy: PlayerCombatPolicy) -> Self {
        self.player_combat = policy;
        self.fingerprint = self.compute_fingerprint();
        self
    }

    #[must_use]
    pub const fn player_combat_policy(&self) -> PlayerCombatPolicy {
        self.player_combat
    }

    #[must_use]
    pub const fn revision(&self) -> u64 {
        self.definition.revision()
    }

    /// Stable FNV-1a 64 hash of the canonical encoding of every table.
    #[must_use]
    pub const fn fingerprint(&self) -> u64 {
        self.fingerprint
    }

    #[must_use]
    pub const fn definition(&self) -> &ZoneDefinition {
        &self.definition
    }

    #[must_use]
    pub const fn areas(&self) -> &ZoneAreas {
        &self.areas
    }

    /// Ordered by ID.
    #[must_use]
    pub fn creature_templates(&self) -> &[CreatureTemplate] {
        &self.templates
    }

    #[must_use]
    pub fn creature_template(&self, id: CreatureTemplateId) -> Option<&CreatureTemplate> {
        self.templates
            .binary_search_by_key(&id, |template| template.id)
            .ok()
            .map(|index| &self.templates[index])
    }

    /// Ordered by ID.
    #[must_use]
    pub fn creature_spawns(&self) -> &[CreatureSpawn] {
        &self.spawns
    }

    #[must_use]
    pub fn creature_spawn(&self, id: CreatureId) -> Option<&CreatureSpawn> {
        self.spawns
            .binary_search_by_key(&id, |spawn| spawn.id)
            .ok()
            .map(|index| &self.spawns[index])
    }

    /// Ordered by ID.
    #[must_use]
    pub fn npcs(&self) -> &[Npc] {
        &self.npcs
    }

    #[must_use]
    pub fn npc(&self, id: NpcId) -> Option<&Npc> {
        self.npcs
            .binary_search_by_key(&id, |npc| npc.id)
            .ok()
            .map(|index| &self.npcs[index])
    }

    /// Feet position on y = 0 where released spirits return.
    #[must_use]
    pub const fn graveyard(&self) -> [i32; 2] {
        self.graveyard
    }

    /// An authored RNG seed is part of recovery identity, while economy-only
    /// content changes may intentionally preserve the existing random stream.
    #[must_use]
    pub fn with_rng_seed(mut self, seed: u64) -> Self {
        self.rng_seed = seed;
        self.fingerprint = self.compute_fingerprint();
        self
    }

    #[must_use]
    pub const fn rng_seed(&self) -> u64 {
        self.rng_seed
    }

    /// Binds validated loot rules to recovery identity without changing the
    /// simulation seed. Missing templates have no loot table. Revision zero
    /// is reserved for content that has never bound a loot catalog.
    pub fn with_loot_tables(
        mut self,
        revision: u64,
        mut tables: Vec<(CreatureTemplateId, LootTable)>,
    ) -> Result<Self, ZoneError> {
        if revision == 0 {
            return Err(ZoneError::new("loot catalog revision must be nonzero"));
        }
        if tables.len() > MAX_CREATURE_TEMPLATES {
            return Err(ZoneError::new("zone loot content capacity reached"));
        }
        tables.sort_by_key(|(template, _)| *template);
        if tables.windows(2).any(|pair| pair[0].0 == pair[1].0) {
            return Err(ZoneError::new("duplicate loot template id"));
        }
        if tables
            .iter()
            .any(|(template, _)| self.creature_template(*template).is_none())
        {
            return Err(ZoneError::new("loot table names an unknown template"));
        }
        self.loot_revision = revision;
        self.loot_tables = tables;
        self.fingerprint = self.compute_fingerprint();
        Ok(self)
    }

    #[must_use]
    pub const fn loot_revision(&self) -> u64 {
        self.loot_revision
    }

    /// Ordered by creature template ID; outcome order remains authored.
    #[must_use]
    pub fn loot_tables(&self) -> &[(CreatureTemplateId, LootTable)] {
        &self.loot_tables
    }

    #[must_use]
    pub fn loot_table(&self, template: CreatureTemplateId) -> Option<&LootTable> {
        self.loot_tables
            .binary_search_by_key(&template, |(template, _)| *template)
            .ok()
            .map(|index| &self.loot_tables[index].1)
    }

    /// Binds the ability catalog and each creature template's ability to
    /// recovery identity without changing the simulation seed. Templates
    /// without a binding use no ability. Revision zero is reserved for
    /// content that has never bound the ability catalog; its fingerprint
    /// omits the catalog, so its zones refuse classes and abilities.
    pub fn with_creature_abilities(
        mut self,
        revision: u64,
        mut bindings: Vec<(CreatureTemplateId, AbilityId)>,
    ) -> Result<Self, ZoneError> {
        if revision == 0 {
            return Err(ZoneError::new("ability catalog revision must be nonzero"));
        }
        if bindings.len() > MAX_CREATURE_TEMPLATES {
            return Err(ZoneError::new("zone ability content capacity reached"));
        }
        bindings.sort_by_key(|(template, _)| *template);
        if bindings.windows(2).any(|pair| pair[0].0 == pair[1].0) {
            return Err(ZoneError::new("duplicate creature ability template id"));
        }
        if bindings
            .iter()
            .any(|(template, _)| self.creature_template(*template).is_none())
        {
            return Err(ZoneError::new("creature ability names an unknown template"));
        }
        if bindings.iter().any(|(_, ability)| {
            ability_by_id(*ability).is_none_or(|ability| ability.user != AbilityUser::Creature)
        }) {
            return Err(ZoneError::new("creature ability names no creature ability"));
        }
        self.ability_revision = revision;
        self.creature_abilities = bindings;
        self.fingerprint = self.compute_fingerprint();
        Ok(self)
    }

    #[must_use]
    pub const fn ability_revision(&self) -> u64 {
        self.ability_revision
    }

    /// Ordered by creature template ID.
    #[must_use]
    pub fn creature_abilities(&self) -> &[(CreatureTemplateId, AbilityId)] {
        &self.creature_abilities
    }

    /// The ability creatures of `template` use, if content binds one.
    #[must_use]
    pub fn creature_ability(&self, template: CreatureTemplateId) -> Option<AbilityId> {
        self.creature_abilities
            .binary_search_by_key(&template, |(template, _)| *template)
            .ok()
            .map(|index| self.creature_abilities[index].1)
    }

    /// Binds vendor stock to NPCs with the `Vendor` role and the vendor
    /// rules (sale values) to recovery identity, without changing the
    /// simulation seed. Revision zero is reserved for content without
    /// vendors; its fingerprint omits them, so its zones refuse every trade.
    pub fn with_vendors(
        mut self,
        revision: u64,
        mut vendors: Vec<(NpcId, VendorStock)>,
    ) -> Result<Self, ZoneError> {
        if revision == 0 {
            return Err(ZoneError::new("vendor catalog revision must be nonzero"));
        }
        if vendors.len() > MAX_NPCS {
            return Err(ZoneError::new("zone vendor content capacity reached"));
        }
        vendors.sort_by_key(|(npc, _)| *npc);
        if vendors.windows(2).any(|pair| pair[0].0 == pair[1].0) {
            return Err(ZoneError::new("duplicate vendor npc id"));
        }
        if vendors
            .iter()
            .any(|(npc, _)| self.npc(*npc).is_none_or(|npc| npc.role != NpcRole::Vendor))
        {
            return Err(ZoneError::new("vendor stock names no vendor npc"));
        }
        self.vendor_revision = revision;
        self.vendors = vendors;
        self.fingerprint = self.compute_fingerprint();
        Ok(self)
    }

    #[must_use]
    pub const fn vendor_revision(&self) -> u64 {
        self.vendor_revision
    }

    /// Ordered by NPC ID.
    #[must_use]
    pub fn vendors(&self) -> &[(NpcId, VendorStock)] {
        &self.vendors
    }

    /// The stock `npc` sells, if content binds one.
    #[must_use]
    pub fn vendor_stock(&self, npc: NpcId) -> Option<&VendorStock> {
        self.vendors
            .binary_search_by_key(&npc, |(npc, _)| *npc)
            .ok()
            .map(|index| &self.vendors[index].1)
    }

    /// Binds quests, validated against this content, and the quest rules to
    /// recovery identity without changing the simulation seed. Revision zero
    /// is reserved for content without quests; its zones refuse every quest.
    pub fn with_quests(
        mut self,
        revision: u64,
        quests: Vec<crate::Quest>,
    ) -> Result<Self, ZoneError> {
        if revision == 0 {
            return Err(ZoneError::new("quest catalog revision must be nonzero"));
        }
        let quests = crate::quest::validate_quests(&self, quests)
            .map_err(|error| ZoneError::new(error.to_string()))?;
        self.quest_revision = revision;
        self.quests = quests;
        self.fingerprint = self.compute_fingerprint();
        Ok(self)
    }

    #[must_use]
    pub const fn quest_revision(&self) -> u64 {
        self.quest_revision
    }

    /// Ordered by quest ID.
    #[must_use]
    pub fn quests(&self) -> &[crate::Quest] {
        &self.quests
    }

    #[must_use]
    pub fn quest(&self, id: crate::QuestId) -> Option<&crate::Quest> {
        self.quests
            .binary_search_by_key(&id, |quest| quest.id)
            .ok()
            .map(|index| &self.quests[index])
    }

    /// Binds doors (#158): unique IDs, leaf bodies in the static collider namespace that no
    /// content collider uses, and non-empty leaf boxes inside the content range.
    pub fn with_doors(mut self, mut doors: Vec<crate::Door>) -> Result<Self, ZoneError> {
        if doors.len() > crate::MAX_DOORS {
            return Err(ZoneError::new("zone door content capacity reached"));
        }
        doors.sort_by_key(|door| door.id);
        if doors
            .windows(2)
            .any(|pair| pair[0].id == pair[1].id || pair[0].body == pair[1].body)
        {
            return Err(ZoneError::new("duplicate door id or body"));
        }
        for door in &doors {
            if u64::from(door.body) >= crate::entity::PLAYER_BODY_BASE
                || self
                    .definition
                    .colliders()
                    .iter()
                    .any(|c| c.id == door.body)
            {
                return Err(ZoneError::new("door body collides with another body id"));
            }
            for leaf in [door.closed, door.open] {
                if leaf.half_extents.iter().any(|half| *half <= 0)
                    || (0..3).any(|axis| {
                        (leaf.position[axis] - leaf.half_extents[axis]).abs()
                            > super::MAX_CONTENT_COORDINATE_UNITS
                            || (leaf.position[axis] + leaf.half_extents[axis]).abs()
                                > super::MAX_CONTENT_COORDINATE_UNITS
                    })
                {
                    return Err(ZoneError::new("door leaf box is empty or out of range"));
                }
            }
        }
        self.doors = doors;
        self.fingerprint = self.compute_fingerprint();
        Ok(self)
    }

    /// Binds moving platforms (#56): unique IDs, bodies in the static collider namespace that
    /// no collider or door uses, valid boxes and schedules.
    pub fn with_platforms(
        mut self,
        mut platforms: Vec<crate::Platform>,
    ) -> Result<Self, ZoneError> {
        if platforms.len() > crate::MAX_PLATFORMS {
            return Err(ZoneError::new("zone platform content capacity reached"));
        }
        platforms.sort_by_key(|platform| platform.id);
        if platforms
            .windows(2)
            .any(|pair| pair[0].id == pair[1].id || pair[0].body == pair[1].body)
        {
            return Err(ZoneError::new("duplicate platform id or body"));
        }
        for platform in &platforms {
            platform.validate()?;
            if u64::from(platform.body) >= crate::entity::PLAYER_BODY_BASE
                || self
                    .definition
                    .colliders()
                    .iter()
                    .any(|c| c.id == platform.body)
                || self.doors.iter().any(|door| door.body == platform.body)
            {
                return Err(ZoneError::new(
                    "platform body collides with another body id",
                ));
            }
        }
        self.platforms = platforms;
        self.fingerprint = self.compute_fingerprint();
        Ok(self)
    }

    /// Binds ramps (#59): bodies in the static collider namespace that no collider, door,
    /// platform or other ramp uses, with valid boxes. Ramps are ordered by body.
    pub fn with_ramps(mut self, mut ramps: Vec<crate::Ramp>) -> Result<Self, ZoneError> {
        if ramps.len() > crate::MAX_RAMPS {
            return Err(ZoneError::new("zone ramp content capacity reached"));
        }
        ramps.sort_by_key(|ramp| ramp.body);
        if ramps.windows(2).any(|pair| pair[0].body == pair[1].body) {
            return Err(ZoneError::new("duplicate ramp body"));
        }
        for ramp in &ramps {
            ramp.validate()?;
            if u64::from(ramp.body) >= crate::entity::PLAYER_BODY_BASE
                || self
                    .definition
                    .colliders()
                    .iter()
                    .any(|c| c.id == ramp.body)
                || self.doors.iter().any(|door| door.body == ramp.body)
                || self
                    .platforms
                    .iter()
                    .any(|platform| platform.body == ramp.body)
            {
                return Err(ZoneError::new("ramp body collides with another body id"));
            }
        }
        self.ramps = ramps;
        self.fingerprint = self.compute_fingerprint();
        Ok(self)
    }

    /// Binds destructible structures (#305): sites and pieces in ID order, piece bodies in the
    /// static collider namespace that nothing else uses, supports among earlier pieces.
    pub fn with_structures(
        mut self,
        sites: Vec<crate::StructureSite>,
        pieces: Vec<crate::StructurePiece>,
    ) -> Result<Self, ZoneError> {
        crate::structure::validate(&sites, &pieces)?;
        let mut bodies: Vec<_> = pieces.iter().map(|piece| piece.body).collect();
        bodies.sort_unstable();
        if bodies.windows(2).any(|pair| pair[0] == pair[1])
            || bodies.iter().any(|&body| {
                u64::from(body) >= crate::entity::PLAYER_BODY_BASE
                    || self.definition.colliders().iter().any(|c| c.id == body)
                    || self.doors.iter().any(|door| door.body == body)
                    || self.platforms.iter().any(|platform| platform.body == body)
                    || self.ramps.iter().any(|ramp| ramp.body == body)
            })
        {
            return Err(ZoneError::new(
                "structure piece body collides with another body id",
            ));
        }
        self.sites = sites;
        self.pieces = pieces;
        self.fingerprint = self.compute_fingerprint();
        Ok(self)
    }

    /// Destructible sites in ID order (#305).
    #[must_use]
    pub fn sites(&self) -> &[crate::StructureSite] {
        &self.sites
    }

    /// Destructible pieces in ID order (#305).
    #[must_use]
    pub fn pieces(&self) -> &[crate::StructurePiece] {
        &self.pieces
    }

    /// Ordered by body.
    #[must_use]
    pub fn ramps(&self) -> &[crate::Ramp] {
        &self.ramps
    }

    /// Ordered by platform ID.
    #[must_use]
    pub fn platforms(&self) -> &[crate::Platform] {
        &self.platforms
    }

    /// Ordered by door ID.
    #[must_use]
    pub fn doors(&self) -> &[crate::Door] {
        &self.doors
    }

    #[must_use]
    pub fn door(&self, id: crate::DoorId) -> Option<&crate::Door> {
        self.doors
            .binary_search_by_key(&id, |door| door.id)
            .ok()
            .map(|index| &self.doors[index])
    }

    /// Binds NPC day/night routines and the vendors that close at night (#158): each routine
    /// names a distinct content NPC, has 1..=[`crate::MAX_ROUTINE_WAYPOINTS`] in-range
    /// waypoints and at most a content door; each closing NPC is a content vendor.
    pub fn with_routines(
        mut self,
        mut routines: Vec<crate::NpcRoutine>,
        mut closed_at_night: Vec<NpcId>,
    ) -> Result<Self, ZoneError> {
        routines.sort_by_key(|routine| routine.npc);
        closed_at_night.sort();
        if routines.windows(2).any(|pair| pair[0].npc == pair[1].npc)
            || closed_at_night.windows(2).any(|pair| pair[0] == pair[1])
        {
            return Err(ZoneError::new("duplicate npc routine or closing vendor"));
        }
        for routine in &routines {
            if self.npc(routine.npc).is_none()
                || routine.path.is_empty()
                || routine.path.len() > crate::MAX_ROUTINE_WAYPOINTS
                || routine
                    .path
                    .iter()
                    .flatten()
                    .any(|value| value.abs() > super::MAX_CONTENT_COORDINATE_UNITS)
                || routine.door.is_some_and(|door| self.door(door).is_none())
            {
                return Err(ZoneError::new("invalid npc routine"));
            }
        }
        if closed_at_night
            .iter()
            .any(|npc| self.vendor_stock(*npc).is_none())
        {
            return Err(ZoneError::new("a closing npc is not a vendor"));
        }
        self.routines = routines;
        self.closed_at_night = closed_at_night;
        self.fingerprint = self.compute_fingerprint();
        Ok(self)
    }

    /// Ordered by NPC ID.
    #[must_use]
    pub fn routines(&self) -> &[crate::NpcRoutine] {
        &self.routines
    }

    #[must_use]
    pub fn routine(&self, npc: NpcId) -> Option<&crate::NpcRoutine> {
        self.routines
            .binary_search_by_key(&npc, |routine| routine.npc)
            .ok()
            .map(|index| &self.routines[index])
    }

    /// Whether the vendor `npc` refuses trade between dusk and dawn.
    #[must_use]
    pub fn closed_at_night(&self, npc: NpcId) -> bool {
        self.closed_at_night.binary_search(&npc).is_ok()
    }

    fn compute_fingerprint(&self) -> u64 {
        let fingerprint = self.compute_structure_fingerprint();
        if self.player_combat == PlayerCombatPolicy::Safe {
            return fingerprint;
        }
        let mut hash = Fnv1a::new();
        hash.bytes(b"mmorpg.zone-content/player-combat-v1");
        hash.u64(fingerprint);
        hash.u8(1);
        hash.finish()
    }

    fn compute_structure_fingerprint(&self) -> u64 {
        let fingerprint = self.compute_ramp_fingerprint();
        if self.pieces.is_empty() {
            return fingerprint;
        }
        let mut hash = Fnv1a::new();
        hash.bytes(b"mmorpg.zone-content/v11");
        hash.u64(fingerprint);
        hash.len(self.sites.len());
        for site in &self.sites {
            hash.u8(site.id);
            hash.text(&site.name);
            hash.u32(site.regrowth_ticks);
        }
        hash.len(self.pieces.len());
        for piece in &self.pieces {
            hash.u32(u32::from(piece.id.get()));
            hash.u8(piece.site);
            hash.u32(piece.body);
            for value in piece.position.iter().chain(&piece.half_extents) {
                hash.i32(*value);
            }
            hash.u32(piece.health);
            hash.len(piece.supports.len());
            for support in &piece.supports {
                hash.u32(u32::from(support.get()));
            }
            match piece.rubble {
                None => hash.u8(0),
                Some((centre, half)) => {
                    hash.u8(1);
                    for value in centre.iter().chain(&half) {
                        hash.i32(*value);
                    }
                }
            }
        }
        hash.finish()
    }

    fn compute_ramp_fingerprint(&self) -> u64 {
        let fingerprint = self.compute_platform_fingerprint();
        if self.ramps.is_empty() {
            return fingerprint;
        }
        let mut hash = Fnv1a::new();
        hash.bytes(b"mmorpg.zone-content/v10");
        hash.u64(fingerprint);
        hash.len(self.ramps.len());
        for ramp in &self.ramps {
            hash.u32(ramp.body);
            for value in ramp.position.iter().chain(&ramp.half_extents) {
                hash.i32(*value);
            }
            hash.u8(ramp.rises.code());
        }
        hash.finish()
    }

    fn compute_platform_fingerprint(&self) -> u64 {
        let fingerprint = self.compute_routine_fingerprint();
        if self.platforms.is_empty() {
            return fingerprint;
        }
        let mut hash = Fnv1a::new();
        hash.bytes(b"mmorpg.zone-content/v9");
        hash.u64(fingerprint);
        hash.len(self.platforms.len());
        for platform in &self.platforms {
            hash.u8(platform.id.get());
            hash.u32(platform.body);
            for value in platform
                .half_extents
                .iter()
                .chain(&platform.low)
                .chain(&platform.high)
            {
                hash.i32(*value);
            }
            hash.i32(platform.speed);
            hash.u32(platform.dwell);
        }
        hash.finish()
    }

    fn compute_routine_fingerprint(&self) -> u64 {
        let fingerprint = self.compute_door_fingerprint();
        if self.routines.is_empty() && self.closed_at_night.is_empty() {
            return fingerprint;
        }
        let mut hash = Fnv1a::new();
        hash.bytes(b"mmorpg.zone-content/v8");
        hash.u64(fingerprint);
        hash.len(self.routines.len());
        for routine in &self.routines {
            hash.u32(routine.npc.get());
            hash.len(routine.path.len());
            for value in routine.path.iter().flatten() {
                hash.i32(*value);
            }
            hash.u16(routine.home_facing);
            hash.u8(routine.door.map_or(0, crate::DoorId::get));
        }
        hash.len(self.closed_at_night.len());
        for npc in &self.closed_at_night {
            hash.u32(npc.get());
        }
        hash.finish()
    }

    fn compute_door_fingerprint(&self) -> u64 {
        let fingerprint = self.compute_quest_fingerprint();
        if self.doors.is_empty() {
            return fingerprint;
        }
        let mut hash = Fnv1a::new();
        hash.bytes(b"mmorpg.zone-content/v7");
        hash.u64(fingerprint);
        hash.len(self.doors.len());
        for door in &self.doors {
            hash.u8(door.id.get());
            hash.u32(door.body);
            for leaf in [door.closed, door.open] {
                for value in leaf.position.iter().chain(&leaf.half_extents) {
                    hash.i32(*value);
                }
            }
        }
        hash.finish()
    }

    fn compute_quest_fingerprint(&self) -> u64 {
        let fingerprint = self.compute_vendor_fingerprint();
        if self.quest_revision == 0 {
            return fingerprint;
        }
        let mut hash = Fnv1a::new();
        hash.bytes(b"mmorpg.zone-content/v6");
        hash.u64(fingerprint);
        hash.u64(self.quest_revision);
        hash.len(self.quests.len());
        for quest in &self.quests {
            hash.u8(quest.id.get());
            hash.text(&quest.name);
            hash.text(&quest.text);
            hash.u32(quest.giver.get());
            hash.u32(quest.ender.get());
            hash.u8(quest.prerequisite.map_or(0, crate::QuestId::get));
            hash.len(quest.objectives.len());
            for objective in &quest.objectives {
                hash.u8(objective.code());
                match *objective {
                    crate::QuestObjective::Kill { template, count } => {
                        hash.u16(template.get());
                        hash.u8(count);
                    }
                    crate::QuestObjective::Collect {
                        item,
                        count,
                        source,
                    } => {
                        hash.u16(item.get());
                        hash.u8(count);
                        hash.u16(source.get());
                    }
                    crate::QuestObjective::Talk { npc } => hash.u32(npc.get()),
                    crate::QuestObjective::Explore { area } => hash.u16(area.get()),
                }
            }
            hash.u32(quest.rewards.experience);
            hash.u32(quest.rewards.copper);
            hash.len(quest.rewards.choices.len());
            for item in &quest.rewards.choices {
                hash.u16(item.get());
            }
        }
        hash.finish()
    }

    fn compute_vendor_fingerprint(&self) -> u64 {
        let fingerprint = self.compute_ability_fingerprint();
        if self.vendor_revision == 0 {
            return fingerprint;
        }
        let mut hash = Fnv1a::new();
        hash.bytes(b"mmorpg.zone-content/v5");
        hash.u64(fingerprint);
        hash.u64(self.vendor_revision);
        for item in crate::ITEM_CATALOG {
            hash.u16(item.id.get());
            hash.u32(crate::sell_price(item.id));
        }
        hash.len(self.vendors.len());
        for (npc, stock) in &self.vendors {
            hash.u32(npc.get());
            hash.len(stock.offers().len());
            for offer in stock.offers() {
                hash.u16(offer.item.get());
                hash.u32(offer.price);
            }
        }
        hash.finish()
    }

    fn compute_ability_fingerprint(&self) -> u64 {
        let fingerprint = self.compute_loot_fingerprint();
        if self.ability_revision == 0 {
            return fingerprint;
        }
        let mut hash = Fnv1a::new();
        hash.bytes(b"mmorpg.zone-content/v4");
        hash.u64(fingerprint);
        hash.u64(self.ability_revision);
        hash.bytes(&crate::ability::catalog_bytes());
        hash.len(self.creature_abilities.len());
        for (template, ability) in &self.creature_abilities {
            hash.u16(template.get());
            hash.u8(ability.get());
        }
        hash.finish()
    }

    fn compute_loot_fingerprint(&self) -> u64 {
        let mut hash = Fnv1a::new();
        hash.bytes(b"mmorpg.zone-content/v2");
        hash.u64(self.compute_simulation_fingerprint());
        hash.u64(self.rng_seed);
        hash.u64(crate::ITEM_CATALOG_REVISION);
        hash.len(crate::ITEM_CATALOG.len());
        for item in crate::ITEM_CATALOG {
            hash.u16(item.id.get());
            hash.text(item.name);
            hash.u16(item.max_stack);
            let stats = item.stats;
            hash.bytes(&[
                item.slot.map_or(0, |slot| 1 + slot.index()),
                stats.stamina,
                stats.strength,
                stats.agility,
                stats.intellect,
            ]);
        }
        for slot in crate::Inventory::starter().slots() {
            hash.u16(slot.map_or(0, |stack| stack.item().get()));
            hash.u16(slot.map_or(0, |stack| stack.quantity()));
        }
        let base_fingerprint = hash.finish();
        if self.loot_revision == 0 {
            return base_fingerprint;
        }
        let mut hash = Fnv1a::new();
        hash.bytes(b"mmorpg.zone-content/v3");
        hash.u64(base_fingerprint);
        hash.u64(self.loot_revision);
        hash.len(self.loot_tables.len());
        for (template, table) in &self.loot_tables {
            hash.u16(template.get());
            for bound in table.money_range() {
                hash.u32(bound);
            }
            hash.len(table.outcomes().len());
            for outcome in table.outcomes() {
                match *outcome {
                    LootOutcome::Nothing { weight } => {
                        hash.u8(0);
                        hash.u16(weight);
                    }
                    LootOutcome::Item {
                        weight,
                        item,
                        quantity,
                    } => {
                        hash.u8(1);
                        hash.u16(weight);
                        hash.u16(item.get());
                        hash.u16(quantity[0]);
                        hash.u16(quantity[1]);
                    }
                }
            }
        }
        hash.finish()
    }

    // Preserve the pre-inventory default seed for unchanged physical/unit content.
    fn compute_simulation_fingerprint(&self) -> u64 {
        let mut hash = Fnv1a::new();
        hash.bytes(b"mmorpg.zone-content/v1");
        let definition = &self.definition;
        hash.u64(definition.revision());
        hash.i32s(&definition.gravity());
        let grid = definition.spawn_grid();
        hash.i32s(&grid.origin);
        hash.u16(grid.columns);
        hash.i32(grid.spacing);
        hash.len(definition.colliders().len());
        for collider in definition.colliders() {
            hash.u32(collider.id);
            hash.i32s(&collider.position);
            hash.i32s(&collider.half_extents);
        }
        if let Some(terrain) = definition.terrain() {
            hash.bytes(b"terrain");
            hash.i32s(&terrain.origin);
            hash.i32(terrain.step);
            hash.u16(terrain.columns);
            hash.u16(terrain.rows);
            hash.i32s(&terrain.heights);
        }
        hash.len(self.areas.areas().len());
        for area in self.areas.areas() {
            hash.u16(area.id().get());
            hash.text(area.name());
            hash.i32s(&area.min_xz());
            hash.i32s(&area.max_xz());
        }
        hash.len(self.templates.len());
        for template in &self.templates {
            hash.u16(template.id.get());
            hash.text(&template.name);
            hash.u8(family_code(template.family));
            hash.u8(match template.behaviour {
                super::units::CreatureBehaviour::Aggressive => 1,
                super::units::CreatureBehaviour::Neutral => 2,
                super::units::CreatureBehaviour::Prop(kind) => 16 + kind.code(),
            });
            hash.u8(template.min_level);
            hash.u8(template.max_level);
            hash.u8(u8::from(template.elite));
            hash.u32(template.health);
            hash.u32(template.health_per_level);
            hash.u16(template.damage[0]);
            hash.u16(template.damage[1]);
            hash.u16(template.damage_per_level);
            hash.u16(template.swing_ticks);
            hash.u32(template.respawn_ticks);
            hash.i32s(&template.half_extents);
        }
        hash.len(self.spawns.len());
        for spawn in &self.spawns {
            hash.u32(spawn.id.get());
            hash.u16(spawn.template.get());
            hash.i32s(&spawn.position);
            hash.u16(spawn.facing);
            hash.i32(spawn.wander_radius);
        }
        hash.len(self.npcs.len());
        for npc in &self.npcs {
            hash.u32(npc.id.get());
            hash.text(&npc.name);
            hash.u8(match npc.role {
                super::units::NpcRole::QuestGiver => 1,
                super::units::NpcRole::Vendor => 2,
                super::units::NpcRole::SpiritHealer => 3,
                super::units::NpcRole::Guard => 4,
            });
            hash.u8(npc.level);
            hash.i32s(&npc.position);
            hash.u16(npc.facing);
        }
        hash.i32s(&self.graveyard);
        hash.finish()
    }
}

const fn family_code(family: super::units::CreatureFamily) -> u8 {
    use super::units::CreatureFamily;
    match family {
        CreatureFamily::Wolf => 1,
        CreatureFamily::Boar => 2,
        CreatureFamily::Vermin => 3,
        CreatureFamily::Marauder => 4,
        CreatureFamily::Mirefin => 5,
        CreatureFamily::Redbrand => 6,
        CreatureFamily::Prop => 7,
        CreatureFamily::Ogre => 8,
    }
}

fn validate_name(name: &str) -> Result<(), ZoneError> {
    if name.trim().is_empty() || name.len() > MAX_UNIT_NAME_BYTES {
        return Err(ZoneError::new("unit name must be 1..=64 bytes of text"));
    }
    Ok(())
}

fn validate_template(template: &CreatureTemplate) -> Result<(), ZoneError> {
    validate_name(&template.name)?;
    // Breakable props (#288) are exactly the prop family.
    if template.prop().is_some() != (template.family == super::units::CreatureFamily::Prop) {
        return Err(ZoneError::new(
            "creature template prop behaviour and family disagree",
        ));
    }
    if template.min_level == 0
        || template.min_level > template.max_level
        || template.max_level > MAX_UNIT_LEVEL
    {
        return Err(ZoneError::new("creature template levels are out of range"));
    }
    let levels = u32::from(template.max_level - template.min_level);
    let top_health = template
        .health_per_level
        .checked_mul(levels)
        .and_then(|bonus| bonus.checked_add(template.health));
    if template.health == 0 || top_health.is_none() {
        return Err(ZoneError::new("creature template health is out of range"));
    }
    let top_damage = u16::try_from(levels)
        .ok()
        .and_then(|levels| template.damage_per_level.checked_mul(levels))
        .and_then(|bonus| template.damage[1].checked_add(bonus));
    if template.damage[0] == 0
        || template.damage[0] > template.damage[1]
        || top_damage.is_none_or(|damage| damage > MAX_SWING_DAMAGE)
    {
        return Err(ZoneError::new("creature template damage is out of range"));
    }
    if template.swing_ticks == 0 || template.respawn_ticks < CORPSE_TICKS {
        return Err(ZoneError::new("creature template timing is out of range"));
    }
    if template
        .half_extents
        .iter()
        .any(|&extent| !(1..=MAX_CREATURE_HALF_EXTENT_UNITS).contains(&extent))
    {
        return Err(ZoneError::new(
            "creature template extents must be positive and bounded",
        ));
    }
    Ok(())
}

/// A body whose feet rest on y = 0 at `feet`.
const fn standing_body(feet: [i32; 2], half_extents: [i32; 3]) -> BodyBox {
    ([feet[0], half_extents[1], feet[1]], half_extents)
}

/// Touching is allowed; overlapping on every axis is not.
fn overlaps(left: BodyBox, right: BodyBox) -> bool {
    (0..3).all(|axis| {
        let distance = (i64::from(left.0[axis]) - i64::from(right.0[axis])).abs();
        distance < i64::from(left.1[axis]) + i64::from(right.1[axis])
    })
}

fn spawn_slot_boxes(definition: &ZoneDefinition) -> Result<Vec<BodyBox>, ZoneError> {
    (0..MAX_PLAYERS_PER_ZONE)
        .map(|slot| {
            u16::try_from(slot)
                .ok()
                .and_then(|slot| definition.spawn_grid().feet(slot))
                .map(|feet| standing_body([feet[0], feet[2]], PLAYER_HALF_EXTENTS_UNITS))
                .ok_or_else(|| ZoneError::new("spawn grid overflows"))
        })
        .collect()
}

fn validate_unit_body(
    definition: &ZoneDefinition,
    body: BodyBox,
    units: &[BodyBox],
    spawn_slots: &[BodyBox],
) -> Result<(), ZoneError> {
    let inside = (0..3).all(|axis| {
        let (Some(min), Some(max)) = (
            body.0[axis].checked_sub(body.1[axis]),
            body.0[axis].checked_add(body.1[axis]),
        ) else {
            return false;
        };
        -MAX_CONTENT_COORDINATE_UNITS <= min && max <= MAX_CONTENT_COORDINATE_UNITS
    });
    if !inside {
        return Err(ZoneError::new(
            "unit body lies outside the content coordinate range",
        ));
    }
    if definition
        .colliders()
        .iter()
        .any(|collider| overlaps(body, (collider.position, collider.half_extents)))
    {
        return Err(ZoneError::new("unit body overlaps a static collider"));
    }
    if units.iter().any(|&other| overlaps(body, other)) {
        return Err(ZoneError::new("unit body overlaps another unit"));
    }
    if spawn_slots.iter().any(|&slot| overlaps(body, slot)) {
        return Err(ZoneError::new("unit body overlaps a player spawn slot"));
    }
    Ok(())
}

/// FNV-1a 64 over big-endian fields; lengths prefix every list and string.
struct Fnv1a(u64);

impl Fnv1a {
    const fn new() -> Self {
        Self(0xcbf2_9ce4_8422_2325)
    }

    fn bytes(&mut self, bytes: &[u8]) {
        for byte in bytes {
            self.0 ^= u64::from(*byte);
            self.0 = self.0.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }

    fn u8(&mut self, value: u8) {
        self.bytes(&[value]);
    }

    fn u16(&mut self, value: u16) {
        self.bytes(&value.to_be_bytes());
    }

    fn u32(&mut self, value: u32) {
        self.bytes(&value.to_be_bytes());
    }

    fn u64(&mut self, value: u64) {
        self.bytes(&value.to_be_bytes());
    }

    fn i32(&mut self, value: i32) {
        self.bytes(&value.to_be_bytes());
    }

    fn i32s(&mut self, values: &[i32]) {
        for value in values {
            self.i32(*value);
        }
    }

    fn len(&mut self, length: usize) {
        self.u64(u64::try_from(length).unwrap_or(u64::MAX));
    }

    fn text(&mut self, text: &str) {
        self.len(text.len());
        self.bytes(text.as_bytes());
    }

    const fn finish(&self) -> u64 {
        self.0
    }
}

#[cfg(test)]
mod tests {
    use super::super::units::{CreatureBehaviour, CreatureFamily, NpcRole};
    use super::*;
    use crate::{SpawnGrid, StaticCollider};

    fn ground() -> ZoneDefinition {
        ZoneDefinition::with_spawn_grid(
            9,
            [0, -1, 0],
            SpawnGrid {
                origin: [0, 0],
                columns: 32,
                spacing: 100,
            },
            vec![StaticCollider {
                id: 1,
                position: [0, -50, 0],
                half_extents: [10_000, 50, 10_000],
            }],
        )
        .unwrap()
    }

    fn wolf() -> CreatureTemplate {
        CreatureTemplate {
            id: CreatureTemplateId::new(1),
            name: "Timber Wolf".into(),
            family: CreatureFamily::Wolf,
            behaviour: CreatureBehaviour::Aggressive,
            min_level: 1,
            max_level: 2,
            elite: false,
            health: 42,
            health_per_level: 14,
            damage: [2, 4],
            damage_per_level: 1,
            swing_ticks: 60,
            respawn_ticks: 1_800,
            half_extents: [40, 45, 40],
        }
    }

    fn spawn(id: u32, position: [i32; 2]) -> CreatureSpawn {
        CreatureSpawn {
            id: CreatureId::new(id),
            template: CreatureTemplateId::new(1),
            position,
            facing: 0,
            wander_radius: 600,
        }
    }

    fn npc(id: u32, position: [i32; 2]) -> Npc {
        Npc {
            id: NpcId::new(id),
            name: "Guard".into(),
            role: NpcRole::Guard,
            level: 10,
            position,
            facing: 0,
        }
    }

    fn content(
        templates: Vec<CreatureTemplate>,
        spawns: Vec<CreatureSpawn>,
        npcs: Vec<Npc>,
        graveyard: [i32; 2],
    ) -> Result<ZoneContent, ZoneError> {
        ZoneContent::new(
            ground(),
            ZoneAreas::default(),
            templates,
            spawns,
            npcs,
            graveyard,
        )
    }

    #[test]
    fn tables_are_ordered_and_scaled_by_level() {
        let content = content(
            vec![wolf()],
            vec![spawn(9, [0, 5_000]), spawn(3, [1_000, 5_000])],
            vec![npc(2, [-1_000, 5_000])],
            [0, -1_000],
        )
        .unwrap();
        let ids: Vec<_> = content
            .creature_spawns()
            .iter()
            .map(|spawn| spawn.id.get())
            .collect();
        assert_eq!(ids, [3, 9]);
        let template = content
            .creature_template(CreatureTemplateId::new(1))
            .unwrap();
        assert_eq!(template.max_health(1), 42);
        assert_eq!(template.max_health(2), 56);
        assert_eq!(template.max_health(9), 56, "clamped to the level range");
        assert_eq!(template.damage_at(2), [3, 5]);
        assert!(content.creature_spawn(CreatureId::new(9)).is_some());
        assert!(content.npc(NpcId::new(2)).is_some());
        assert_eq!(content.graveyard(), [0, -1_000]);
    }

    #[test]
    fn invalid_tables_fail_closed() {
        let valid = || (vec![wolf()], vec![spawn(1, [0, 5_000])], vec![]);
        let mut cases: Vec<(&str, Result<ZoneContent, ZoneError>)> = Vec::new();
        let (templates, _, _) = valid();
        cases.push((
            "unknown template",
            content(
                templates,
                vec![CreatureSpawn {
                    template: CreatureTemplateId::new(7),
                    ..spawn(1, [0, 5_000])
                }],
                vec![],
                [0, -1_000],
            ),
        ));
        for mutate in [
            (|template: &mut CreatureTemplate| template.min_level = 0) as fn(&mut CreatureTemplate),
            |template| template.max_level = 0,
            |template| template.health = 0,
            |template| template.health_per_level = u32::MAX,
            |template| template.damage = [5, 4],
            |template| template.damage = [0, 4],
            |template| template.damage = [2, MAX_SWING_DAMAGE],
            |template| template.swing_ticks = 0,
            |template| template.respawn_ticks = CORPSE_TICKS - 1,
            |template| template.half_extents = [0, 45, 40],
            |template| template.name = " ".into(),
        ] {
            let mut template = wolf();
            mutate(&mut template);
            let (_, spawns, npcs) = valid();
            cases.push((
                "template",
                content(vec![template], spawns, npcs, [0, -1_000]),
            ));
        }
        let (templates, _, _) = valid();
        cases.push((
            "overlapping spawns",
            content(
                templates.clone(),
                vec![spawn(1, [0, 5_000]), spawn(2, [50, 5_000])],
                vec![],
                [0, -1_000],
            ),
        ));
        cases.push((
            "spawn on a spawn slot",
            content(
                templates.clone(),
                vec![spawn(1, [0, 0])],
                vec![],
                [0, -1_000],
            ),
        ));
        cases.push((
            "spawn in the ground",
            content(
                templates.clone(),
                vec![spawn(1, [0, 5_000])],
                vec![],
                [0, -1_000],
            )
            .and_then(|_| {
                ZoneContent::new(
                    ZoneDefinition::new(
                        9,
                        [0, -1, 0],
                        vec![StaticCollider {
                            id: 1,
                            position: [0, 50, 5_000],
                            half_extents: [100, 50, 100],
                        }],
                    )?,
                    ZoneAreas::default(),
                    templates.clone(),
                    vec![spawn(1, [0, 5_000])],
                    vec![],
                    [0, -1_000],
                )
            }),
        ));
        cases.push((
            "duplicate npc",
            content(
                templates.clone(),
                vec![],
                vec![npc(1, [0, 5_000]), npc(1, [1_000, 5_000])],
                [0, -1_000],
            ),
        ));
        cases.push((
            "npc on a creature",
            content(
                templates.clone(),
                vec![spawn(1, [0, 5_000])],
                vec![npc(1, [30, 5_000])],
                [0, -1_000],
            ),
        ));
        cases.push((
            "graveyard in an npc",
            content(
                templates.clone(),
                vec![],
                vec![npc(1, [0, 5_000])],
                [0, 5_000],
            ),
        ));
        cases.push((
            "outside the content range",
            content(
                templates.clone(),
                vec![spawn(1, [MAX_CONTENT_COORDINATE_UNITS, 5_000])],
                vec![],
                [0, -1_000],
            ),
        ));
        cases.push((
            "wander radius",
            content(
                templates,
                vec![CreatureSpawn {
                    wander_radius: MAX_WANDER_RADIUS_UNITS + 1,
                    ..spawn(1, [0, 5_000])
                }],
                vec![],
                [0, -1_000],
            ),
        ));
        for (name, result) in cases {
            assert!(result.is_err(), "{name} must be rejected");
        }
    }

    #[test]
    fn the_fingerprint_covers_every_table() {
        let base = content(
            vec![wolf()],
            vec![spawn(1, [0, 5_000])],
            vec![npc(1, [1_000, 5_000])],
            [0, -1_000],
        )
        .unwrap();
        let same = content(
            vec![wolf()],
            vec![spawn(1, [0, 5_000])],
            vec![npc(1, [1_000, 5_000])],
            [0, -1_000],
        )
        .unwrap();
        assert_eq!(base.fingerprint(), same.fingerprint());
        let variants = [
            content(
                vec![CreatureTemplate {
                    swing_ticks: 61,
                    ..wolf()
                }],
                vec![spawn(1, [0, 5_000])],
                vec![npc(1, [1_000, 5_000])],
                [0, -1_000],
            ),
            content(
                vec![wolf()],
                vec![spawn(1, [0, 5_001])],
                vec![npc(1, [1_000, 5_000])],
                [0, -1_000],
            ),
            content(
                vec![wolf()],
                vec![spawn(1, [0, 5_000])],
                vec![Npc {
                    name: "Captain".into(),
                    ..npc(1, [1_000, 5_000])
                }],
                [0, -1_000],
            ),
            content(
                vec![wolf()],
                vec![spawn(1, [0, 5_000])],
                vec![npc(1, [1_000, 5_000])],
                [0, -1_001],
            ),
        ];
        for variant in variants {
            assert_ne!(variant.unwrap().fingerprint(), base.fingerprint());
        }
        let colliders_differ = ZoneContent::from_definition(ZoneDefinition::default());
        let physical = ZoneContent::from_definition(ground());
        assert_ne!(colliders_differ.fingerprint(), physical.fingerprint());
        assert_eq!(physical.graveyard(), [0, 0]);
    }
}
