//! Named review scenarios (#371): a fresh local world that starts the reviewer
//! in one interesting encounter, playable at once and reset to the same start.
//!
//! A scenario is development-only setup, never a rule: it reseeds the hosted
//! content's RNG and admits one ephemeral character record next to existing
//! authored creatures. Combat, AI and collision stay in `mmorpg-core`, and no
//! record reaches the page's character storage. Network hosts never see it.

use std::sync::Arc;

use mmorpg_core::{
    CharacterId, CharacterLocation, CharacterRecord, ClassChoice, PLAYER_HALF_EXTENTS_UNITS,
    PlayerClass, Sex, ZoneContent, ZoneSimulation, unit::player_max_health,
};

use crate::host::{LOCAL_ZONE_ID, LocalZoneError, hosted_content};

/// A named starting situation; its name is the page's `?scenario=` value.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Scenario {
    /// A level 5 Warden 15 m north of the Southmeadow Hill Ogre (#297), facing it.
    OgreCombat,
    /// A level 6 Ranger at the eastern edge of Wolfrun Woods, facing the wolf pack among the
    /// trunks, to try Piercing and Ricochet Shot (#291).
    RangerRange,
    /// A level 6 Arcanist south of the Millbrook field marauders, facing them across the
    /// fence, to try Frost Breath, Lightning Beam and Chain Lightning (#292).
    ArcanistPack,
}

impl Scenario {
    pub const ALL: [Self; 3] = [Self::OgreCombat, Self::RangerRange, Self::ArcanistPack];

    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|scenario| scenario.name() == name)
    }

    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::OgreCombat => "ogre-combat",
            Self::RangerRange => "ranger-range",
            Self::ArcanistPack => "arcanist-pack",
        }
    }

    const fn class(self) -> ClassChoice {
        match self {
            Self::OgreCombat => ClassChoice {
                class: PlayerClass::Warden,
                sex: Sex::Female,
            },
            Self::RangerRange => ClassChoice {
                class: PlayerClass::Ranger,
                sex: Sex::Male,
            },
            Self::ArcanistPack => ClassChoice {
                class: PlayerClass::Arcanist,
                sex: Sex::Female,
            },
        }
    }

    /// The ogre is level 5, so the Warden is too.
    const fn level(self) -> u8 {
        match self {
            Self::OgreCombat => 5,
            Self::RangerRange | Self::ArcanistPack => 6,
        }
    }

    /// Ground XZ position in units and facing (+Z is yaw 0).
    pub const fn location(self) -> ([i32; 2], u16) {
        match self {
            // The ogre stands at (7 450, 18 150), outside its 10 m aggro radius.
            Self::OgreCombat => ([7_450, 16_650], 0),
            // The nearest wolves stand about 18 m west, beyond aggro range; facing west.
            Self::RangerRange => ([-3_500, -3_400], 49_152),
            // The marauders' southern row stands at z 8 000; 13 m south, facing north.
            Self::ArcanistPack => ([7_300, 9_300], 32_768),
        }
    }

    /// The hosted content with its RNG seed mixed with `seed`; seed 0 is the
    /// authored world's own seed.
    #[must_use]
    pub fn content(self, seed: u64) -> Arc<ZoneContent> {
        let content = hosted_content();
        let reseeded = content.rng_seed() ^ seed;
        Arc::new(ZoneContent::clone(&content).with_rng_seed(reseeded))
    }

    /// The ephemeral character's record: a freshly created character of the
    /// scenario's class, with its starter gear, raised to the scenario level
    /// at full health and placed at the scenario location. It is built in a
    /// throwaway zone, so the scenario world's RNG is untouched.
    pub fn record(self, content: &Arc<ZoneContent>) -> Result<CharacterRecord, LocalZoneError> {
        let zone_error = LocalZoneError::Zone;
        let mut template =
            ZoneSimulation::with_content(LOCAL_ZONE_ID, Arc::clone(content)).map_err(zone_error)?;
        template
            .create_character(1, self.class())
            .map_err(zone_error)?;
        let id = CharacterId::new(format!("scenario-{}", self.name())).map_err(zone_error)?;
        let mut record = template.capture_character(1, id).map_err(zone_error)?;
        let ([x, z], facing) = self.location();
        // Outer-valley hills are physical (#159): stand on the authored ground, not inside it.
        let ground = content
            .definition()
            .terrain()
            .map_or(0, |terrain| terrain.height_at(x, z));
        record.level = self.level();
        record.experience = 0;
        record.health = player_max_health(record.level) + record.equipment.totals().bonus_health();
        record.location = CharacterLocation {
            zone_id: LOCAL_ZONE_ID,
            position: [x, ground + PLAYER_HALF_EXTENTS_UNITS[1], z],
            facing,
        };
        Ok(record)
    }
}
