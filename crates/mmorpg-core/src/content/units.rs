//! Unit tables of zone content: creature templates, creature spawns and NPCs.
//! They are plain data; [`super::ZoneContent::new`] validates them once.

use crate::{CreatureId, CreatureTemplateId, NpcId};

/// Longest template or NPC name in bytes.
pub const MAX_UNIT_NAME_BYTES: usize = 64;
/// Upper bounds per content revision.
pub const MAX_CREATURE_TEMPLATES: usize = 256;
pub const MAX_CREATURE_SPAWNS: usize = 1_024;
pub const MAX_NPCS: usize = 256;
/// Largest wander radius around a spawn point (20 m).
pub const MAX_WANDER_RADIUS_UNITS: i32 = 2_000;
/// Largest creature half-extent on any axis (5 m).
pub const MAX_CREATURE_HALF_EXTENT_UNITS: i32 = 500;
/// A creature at least 1.4 m across (this half-width) is large (#297): it holds its ground while
/// idle and shoves smaller units it walks into.
pub const LARGE_CREATURE_HALF_WIDTH_UNITS: i32 = 70;

/// Creatures of one family assist each other.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum CreatureFamily {
    Wolf,
    Boar,
    Vermin,
    Marauder,
    Mirefin,
    Redbrand,
    /// Breakable props (#288): crates, barrels and fences. They never assist.
    Prop,
    /// Large creatures (#297): big bodies that body-block, shove smaller units and stomp.
    Ogre,
}

impl CreatureFamily {
    /// Every family, in order.
    pub const ALL: [Self; 8] = [
        Self::Wolf,
        Self::Boar,
        Self::Vermin,
        Self::Marauder,
        Self::Mirefin,
        Self::Redbrand,
        Self::Prop,
        Self::Ogre,
    ];

    /// Stable lower-case name for exports.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Wolf => "wolf",
            Self::Boar => "boar",
            Self::Vermin => "vermin",
            Self::Marauder => "marauder",
            Self::Mirefin => "mirefin",
            Self::Redbrand => "redbrand",
            Self::Prop => "prop",
            Self::Ogre => "ogre",
        }
    }
}

/// What a breakable prop is (#288).
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum PropKind {
    Crate,
    Barrel,
    /// Explodes like Fire Blast when broken, so barrels chain-react.
    ExplosiveBarrel,
    Fence,
}

impl PropKind {
    pub const ALL: [Self; 4] = [
        Self::Crate,
        Self::Barrel,
        Self::ExplosiveBarrel,
        Self::Fence,
    ];

    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Crate => "crate",
            Self::Barrel => "barrel",
            Self::ExplosiveBarrel => "explosive_barrel",
            Self::Fence => "fence",
        }
    }

    /// Stable wire and fingerprint code.
    #[must_use]
    pub const fn code(self) -> u8 {
        match self {
            Self::Crate => 0,
            Self::Barrel => 1,
            Self::ExplosiveBarrel => 2,
            Self::Fence => 3,
        }
    }

    #[must_use]
    pub const fn from_code(code: u8) -> Option<Self> {
        match code {
            0 => Some(Self::Crate),
            1 => Some(Self::Barrel),
            2 => Some(Self::ExplosiveBarrel),
            3 => Some(Self::Fence),
            _ => None,
        }
    }
}

/// Whether a creature starts fights.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum CreatureBehaviour {
    /// Engages the nearest living player within its aggro radius.
    Aggressive,
    /// Engages only when attacked.
    Neutral,
    /// A breakable prop (#288): a targetable unit with a little health that never moves or
    /// fights, gives no experience or loot, and breaks into debris.
    Prop(PropKind),
}

impl CreatureBehaviour {
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Aggressive => "aggressive",
            Self::Neutral => "neutral",
            Self::Prop(_) => "prop",
        }
    }

    /// The prop kind of a breakable.
    #[must_use]
    pub const fn prop(self) -> Option<PropKind> {
        match self {
            Self::Prop(kind) => Some(kind),
            Self::Aggressive | Self::Neutral => None,
        }
    }
}

/// Numbers and shape shared by every creature of one kind. Health and damage
/// are given at `min_level`; each level above it adds the per-level amounts.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CreatureTemplate {
    pub id: CreatureTemplateId,
    pub name: String,
    pub family: CreatureFamily,
    pub behaviour: CreatureBehaviour,
    pub min_level: u8,
    pub max_level: u8,
    pub elite: bool,
    pub health: u32,
    pub health_per_level: u32,
    /// Inclusive melee damage range at `min_level`.
    pub damage: [u16; 2],
    pub damage_per_level: u16,
    pub swing_ticks: u16,
    /// Ticks from death to respawn; at least the corpse duration.
    pub respawn_ticks: u32,
    /// Collision box; its feet rest on y = 0 at the spawn point.
    pub half_extents: [i32; 3],
}

impl CreatureTemplate {
    /// Maximum health at `level`, clamped into the template's level range.
    /// Validation proves the maximum level cannot overflow.
    #[must_use]
    pub const fn max_health(&self, level: u8) -> u32 {
        let above = self.levels_above_min(level);
        self.health + self.health_per_level * above as u32
    }

    /// Inclusive melee damage range at `level`, clamped like [`Self::max_health`].
    #[must_use]
    pub const fn damage_at(&self, level: u8) -> [u16; 2] {
        let bonus = self.damage_per_level * self.levels_above_min(level) as u16;
        [self.damage[0] + bonus, self.damage[1] + bonus]
    }

    /// The prop kind of a breakable template.
    #[must_use]
    pub const fn prop(&self) -> Option<PropKind> {
        self.behaviour.prop()
    }

    /// The half-width of its upright body (the larger horizontal half extent).
    #[must_use]
    pub const fn half_width(&self) -> i32 {
        let [x, _, z] = self.half_extents;
        if x > z { x } else { z }
    }

    /// Whether it is a large creature (#297) that shoves smaller units; props never are.
    #[must_use]
    pub const fn is_large(&self) -> bool {
        self.prop().is_none() && self.half_width() >= LARGE_CREATURE_HALF_WIDTH_UNITS
    }

    /// The collision box of a unit facing `facing`: a facing nearer a quarter turn than a
    /// half turn swaps the X and Z extents, so a fence runs along its facing's side.
    #[must_use]
    pub const fn body_half_extents(&self, facing: u16) -> [i32; 3] {
        let [x, y, z] = self.half_extents;
        if (facing.wrapping_add(8_192) / 16_384) % 2 == 1 {
            [z, y, x]
        } else {
            [x, y, z]
        }
    }

    const fn levels_above_min(&self, level: u8) -> u8 {
        let level = if level > self.max_level {
            self.max_level
        } else {
            level
        };
        level.saturating_sub(self.min_level)
    }
}

/// One creature placement. The creature it hosts has the same ID.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CreatureSpawn {
    pub id: CreatureId,
    pub template: CreatureTemplateId,
    /// Feet position on y = 0, XZ in units.
    pub position: [i32; 2],
    pub facing: u16,
    /// Idle creatures wander within this XZ distance of `position`.
    pub wander_radius: i32,
}

/// What an NPC does for players; later steps give each role behaviour.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum NpcRole {
    QuestGiver,
    Vendor,
    SpiritHealer,
    Guard,
}

impl NpcRole {
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::QuestGiver => "quest_giver",
            Self::Vendor => "vendor",
            Self::SpiritHealer => "spirit_healer",
            Self::Guard => "guard",
        }
    }
}

/// A friendly, unattackable character standing at a post. NPCs have
/// character-sized fixed bodies and no AI yet.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Npc {
    pub id: NpcId,
    pub name: String,
    pub role: NpcRole,
    pub level: u8,
    /// Feet position on y = 0, XZ in units.
    pub position: [i32; 2],
    pub facing: u16,
}
