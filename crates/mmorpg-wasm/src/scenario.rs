//! Named review scenarios (#371): a fresh local world that starts the reviewer
//! in one interesting encounter, playable at once and reset to the same start.
//!
//! A scenario is development-only setup, never a rule: it reseeds the hosted
//! content's RNG and admits one ephemeral character record next to existing
//! authored creatures. Combat, AI and collision stay in `mmorpg-core`, and no
//! record reaches the page's character storage. Network hosts never see it.

use std::sync::Arc;
use strum::VariantArray;

use mmorpg_core::{
    CharacterId, CharacterLocation, CharacterRecord, ClassChoice, PLAYER_HALF_EXTENTS_UNITS,
    PlayerClass, Sex, ZoneContent, ZoneSimulation, unit::player_max_health,
};

use crate::host::{LOCAL_ZONE_ID, LocalZoneError, hosted_content};

/// A named starting situation; its name is the page's `?scenario=` value.
#[derive(Clone, Copy, Debug, Eq, PartialEq, strum::VariantArray)]
pub enum Scenario {
    /// A level 5 Warden 15 m north of the Southmeadow Hill Ogre (#297), facing it.
    OgreCombat,
    /// A level 6 Ranger at the eastern edge of Wolfrun Woods, facing the wolf pack among the
    /// trunks, to try Piercing and Ricochet Shot (#291).
    RangerRange,
    /// A level 8 Arcanist at the eastern edge of Wolfrun Woods, facing the wolf pack, to try
    /// Frost Breath, Lightning Beam and Chain Lightning (#292).
    ArcanistPack,
    /// A level 5 Warden on Stillwater Lake's west shore with *The Sunken Shrine* accepted,
    /// facing the lake: dive to the shrine, swim the cave past its air pocket to the altar,
    /// and meet the lurkers on the way (#274).
    SunkenShrine,
    /// A level 6 Warden before the Greyfang cave mouth with *Deeper into the Hollow* accepted,
    /// facing in: Redbrand Bandits, Crossbowmen and two shield-bearing Brutes hold the outer
    /// cavern and the inner chamber with its sealed threshold (#160).
    RedbrandCavern,
    /// A level 6 Necromancer at the eastern edge of Wolfrun Woods, facing the wolf pack, to try
    /// Shadow Bolt, Corruption, Drain Life, Bone Shield (#301) and Raise Skeleton (#295).
    NecromancerDrain,
    /// A level 6 Rogue at the eastern edge of Wolfrun Woods, facing the wolf pack, to try
    /// Sinister Strike, Poisoned Blade, Gouge and Adrenaline Rush (#302).
    RogueMelee,
    /// A level 6 Cleric at the eastern edge of Wolfrun Woods, facing the wolf pack, to try
    /// Smite, Renew, Holy Beam and Divine Shield (#303).
    ClericHoly,
    /// A level 6 Druid at the eastern edge of Wolfrun Woods, facing the wolf pack, to try
    /// Wrath, Entangling Roots, Rejuvenation and Barkskin (#304).
    DruidNature,
    /// A level 1 Cleric (in the robe whose skirt chains the traversal clips swing) a step south
    /// of a low (0.7 m) Hollowpine boulder, the mill's mid (1.5 m) loading dock or a high (2.1 m)
    /// cottage wall in the Marsh-Lake Wetlands, facing it: walk into it and jump to mantle (#500).
    MantleLow,
    MantleMid,
    MantleHigh,
    /// The same Cleric below the 10 m highland crag on the gorge's eastern rim, facing its southern
    /// face: walk into it, jump to grab it, then climb, traverse and top out (#500).
    Climb,
    /// A level 6 Warden with full rage at the eastern edge of Wolfrun Woods, facing a wolf, to review the
    /// sword-and-shield clips (#582): the directed Heroic Strike's nine slashes and the swing's
    /// slash toward a wolf (low) …
    WardenSlashes,
    /// … the guard lanes' raised shields …
    WardenGuards,
    /// … Charge's rush and shield impact at a wolf …
    WardenCharge,
    /// … and Cleave's sweep.
    WardenCleave,
    /// A level 6 Warden south of the Redbrand Hollow camp, facing the bandits: Shield Bash one to
    /// see it reel (#582).
    WardenStun,
    /// A level 6 Warden on his last hit point beside a Wolfrun wolf: once it kills him, release
    /// the spirit to rise at the graveyard (#582).
    WardenRespawn,
    /// A level 8 Warden a few steps south of Garrick Redbrand at the mine entrance, facing him:
    /// his whirl winds up through its cast and spins (#582).
    RedbrandWhirl,
    /// A level 8 Warden a few steps south of the Greyfang cavern's west Redbrand Brute, facing it:
    /// its overhead smash raises, holds and slams (#582).
    RedbrandSmash,
    /// A level 1 Warden 3 m north of a Greyhaven guard, facing him: both
    /// hands close round the sword and the shield, and the guard's round his sword (#503). The
    /// other classes' grips show in `?scenario=action-lab&class=<class>`.
    Grips,
    /// A level 6 Rogue with full focus 4 m east of a Wolfrun wolf, facing it, to review the
    /// dagger clips (#603): the stance, the alternating stab and slash, Sinister Strike,
    /// Poisoned Blade, Gouge and Adrenaline Rush's flourish …
    RogueDaggers,
    /// … and 13 m from it, the Throwing Knife, Venom Flask and Blasting Bomb throws.
    RogueThrows,
    /// A level 8 Warden 5 m south of a Redbrand Hollow bandit, facing it: beaten below half
    /// health, it wraps a Crude Bandage round its forearm (#603).
    RedbrandBandage,
}

impl Scenario {
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        Self::VARIANTS
            .iter()
            .copied()
            .find(|scenario| scenario.name() == name)
    }

    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::OgreCombat => "ogre-combat",
            Self::RangerRange => "ranger-range",
            Self::ArcanistPack => "arcanist-pack",
            Self::SunkenShrine => "sunken-shrine",
            Self::RedbrandCavern => "redbrand-cavern",
            Self::NecromancerDrain => "necromancer-drain",
            Self::RogueMelee => "rogue-melee",
            Self::ClericHoly => "cleric-holy",
            Self::DruidNature => "druid-nature",
            Self::MantleLow => "mantle-low",
            Self::MantleMid => "mantle-mid",
            Self::MantleHigh => "mantle-high",
            Self::Climb => "climb",
            Self::WardenSlashes => "warden-slashes",
            Self::WardenGuards => "warden-guards",
            Self::WardenCharge => "warden-charge",
            Self::WardenCleave => "warden-cleave",
            Self::WardenStun => "warden-stun",
            Self::WardenRespawn => "warden-respawn",
            Self::RedbrandWhirl => "redbrand-whirl",
            Self::RedbrandSmash => "redbrand-smash",
            Self::Grips => "grips",
            Self::RogueDaggers => "rogue-daggers",
            Self::RogueThrows => "rogue-throws",
            Self::RedbrandBandage => "redbrand-bandage",
        }
    }

    const fn class(self) -> ClassChoice {
        match self {
            Self::OgreCombat | Self::SunkenShrine | Self::RedbrandCavern => ClassChoice {
                class: PlayerClass::Warden,
                sex: Sex::Female,
            },
            // The sword clips were reviewed on the male figure (asset-tooling #447).
            Self::WardenSlashes
            | Self::WardenGuards
            | Self::WardenCharge
            | Self::WardenCleave
            | Self::WardenStun
            | Self::WardenRespawn
            | Self::RedbrandWhirl
            | Self::RedbrandSmash
            | Self::RedbrandBandage
            | Self::Grips => ClassChoice {
                class: PlayerClass::Warden,
                sex: Sex::Male,
            },
            // The dagger clips were reviewed on the male figure (asset-tooling #418).
            Self::RogueDaggers | Self::RogueThrows => ClassChoice {
                class: PlayerClass::Rogue,
                sex: Sex::Male,
            },
            Self::RangerRange => ClassChoice {
                class: PlayerClass::Ranger,
                sex: Sex::Male,
            },
            Self::ArcanistPack => ClassChoice {
                class: PlayerClass::Arcanist,
                sex: Sex::Female,
            },
            Self::NecromancerDrain => ClassChoice {
                class: PlayerClass::Necromancer,
                sex: Sex::Male,
            },
            Self::RogueMelee => ClassChoice {
                class: PlayerClass::Rogue,
                sex: Sex::Female,
            },
            Self::ClericHoly
            | Self::MantleLow
            | Self::MantleMid
            | Self::MantleHigh
            | Self::Climb => ClassChoice {
                class: PlayerClass::Cleric,
                sex: Sex::Male,
            },
            Self::DruidNature => ClassChoice {
                class: PlayerClass::Druid,
                sex: Sex::Female,
            },
        }
    }

    /// The ogre is level 5, so the Warden is too.
    const fn level(self) -> u8 {
        match self {
            Self::OgreCombat | Self::SunkenShrine => 5,
            // Bone Shield, Adrenaline Rush and Divine Shield and Barkskin unlock at level 6.
            Self::RangerRange
            | Self::RedbrandCavern
            | Self::NecromancerDrain
            | Self::RogueMelee
            | Self::ClericHoly
            | Self::DruidNature => 6,
            // Cleave unlocks at level 6.
            Self::WardenSlashes
            | Self::WardenGuards
            | Self::WardenCharge
            | Self::WardenCleave
            | Self::WardenStun
            | Self::WardenRespawn => 6,
            // Adrenaline Rush and Blasting Bomb unlock at level 6.
            Self::RogueDaggers | Self::RogueThrows => 6,
            // Lightning Beam unlocks at level 8; a Warden that high outlasts a Redbrand special.
            Self::ArcanistPack
            | Self::RedbrandWhirl
            | Self::RedbrandSmash
            | Self::RedbrandBandage => 8,
            Self::MantleLow | Self::MantleMid | Self::MantleHigh | Self::Climb | Self::Grips => 1,
        }
    }

    /// Ground XZ position in units and facing (+Z is yaw 0).
    pub const fn location(self) -> ([i32; 2], u16) {
        match self {
            // The ogre stands at (7 450, 18 150), outside its 10 m aggro radius.
            Self::OgreCombat => ([7_450, 16_650], 0),
            // On the woods' rim above the basin (#423); the nearest wolves stand about 15 m
            // west, beyond aggro range; facing west.
            Self::RangerRange => ([-5_000, -4_800], 49_152),
            // Beside the ranger range, nearer the pack's middle; facing west.
            // The Necromancer, the Rogue, the Cleric and the Druid review from the same spot.
            Self::ArcanistPack
            | Self::NecromancerDrain
            | Self::RogueMelee
            | Self::ClericHoly
            | Self::DruidNature
            | Self::WardenGuards => ([-5_000, -2_800], 49_152),
            // 4 m east of Timber Wolf 104's spawn, in swing and Cleave reach once it closes;
            // facing west at it.
            Self::WardenSlashes | Self::WardenCleave | Self::WardenRespawn | Self::RogueDaggers => {
                ([-5_800, -3_600], 49_152)
            }
            // 13 m east of the same wolf, within Charge's 15 m and the throws' range; facing west
            // at it.
            Self::WardenCharge | Self::RogueThrows => ([-4_900, -3_600], 49_152),
            // 12 m south of the camp's nearest bandits; facing north.
            Self::WardenStun => ([0, -6_600], 32_768),
            // 5 m south of Redbrand Bandit 401 at (1 000, -7 800); facing north at it.
            Self::RedbrandBandage => ([1_000, -7_300], 32_768),
            // 5 m south of Garrick; facing north.
            Self::RedbrandWhirl => ([0, -10_000], 32_768),
            // 4 m south of the west Brute (650); facing north toward it.
            Self::RedbrandSmash => ([6_100, -13_300], 32_768),
            // 3 m north of Greyhaven Guard 6 at (-500, -800), who faces north; facing south.
            Self::Grips => ([-500, -1_100], 0),
            // On the west shore, on the lake's long axis; facing east over the water.
            Self::SunkenShrine => ([3_000, -5_400], 16_384),
            // On the rim before Greyfang Ridge (#423), 8 m south of the cave mouth; facing north.
            Self::RedbrandCavern => ([5_800, -12_000], 32_768),
            // 1.5 m south of the ledge, facing north.
            Self::MantleLow => ([-14_530, -890], 32_768),
            Self::MantleMid => ([8_250, 2_250], 32_768),
            Self::MantleHigh => ([36_800, 13_150], 32_768),
            // 3 m south of the crag's southern face; facing north.
            Self::Climb => ([6_800, -29_300], 32_768),
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
        let mut record = fresh_record(
            content,
            self.class(),
            self.level(),
            &format!("scenario-{}", self.name()),
            self.location(),
        )?;
        if self == Self::SunkenShrine {
            use mmorpg_core::greyhaven_vale::quests::{SCOUT_THE_LAKE, THE_SUNKEN_SHRINE};
            // The chain through *Scout the Lake* turned in; the shrine quest accepted.
            record.quests = mmorpg_core::QuestLog {
                entries: vec![mmorpg_core::QuestEntry {
                    quest: THE_SUNKEN_SHRINE,
                    progress: [0; mmorpg_core::MAX_QUEST_OBJECTIVES],
                }],
                completed: (1..=SCOUT_THE_LAKE.get())
                    .filter_map(|id| mmorpg_core::QuestId::new(id).bit())
                    .fold(0, |mask, bit| mask | bit),
            };
        }
        if self == Self::RedbrandCavern {
            use mmorpg_core::greyhaven_vale::quests::{
                DEEPER_INTO_THE_HOLLOW, RETURN_TO_GREYHAVEN,
            };
            // The Garrick chain through *Return to Greyhaven* turned in; quest 11 accepted.
            record.quests = mmorpg_core::QuestLog {
                entries: vec![mmorpg_core::QuestEntry {
                    quest: DEEPER_INTO_THE_HOLLOW,
                    progress: [0; mmorpg_core::MAX_QUEST_OBJECTIVES],
                }],
                completed: (1..=RETURN_TO_GREYHAVEN.get())
                    .filter_map(|id| mmorpg_core::QuestId::new(id).bit())
                    .fold(0, |mask, bit| mask | bit),
            };
        }
        if self.class().class == PlayerClass::Warden && self.level() >= 6 {
            // Full rage, so every Warden ability can be tried at once (#582).
            record.resource = PlayerClass::Warden.resource().max(self.level());
        }
        if matches!(self, Self::RogueDaggers | Self::RogueThrows) {
            // Full focus, so every Rogue ability can be tried at once (#603).
            record.resource = PlayerClass::Rogue.resource().max(self.level());
        }
        if self == Self::WardenRespawn {
            // One hit from the wolf kills; releasing the spirit rises at the graveyard.
            record.health = 1;
        }
        Ok(record)
    }
}

/// A freshly created character of `class` with its starter gear, raised to
/// `level` at full health and standing on the authored ground at
/// `location`. It is built in a throwaway zone, so the hosting world's RNG
/// is untouched.
pub(crate) fn fresh_record(
    content: &Arc<ZoneContent>,
    class: ClassChoice,
    level: u8,
    id: &str,
    ([x, z], facing): ([i32; 2], u16),
) -> Result<CharacterRecord, LocalZoneError> {
    let zone_error = LocalZoneError::Zone;
    let mut template =
        ZoneSimulation::with_content(LOCAL_ZONE_ID, Arc::clone(content)).map_err(zone_error)?;
    template.create_character(1, class).map_err(zone_error)?;
    let id = CharacterId::new(id.to_owned()).map_err(zone_error)?;
    let mut record = template.capture_character(1, id).map_err(zone_error)?;
    // Outer-valley hills are physical (#159): stand on the authored ground, not inside it.
    let ground = content
        .definition()
        .terrain()
        .map_or(0, |terrain| terrain.height_at(x, z));
    record.level = level;
    record.experience = 0;
    record.location = CharacterLocation {
        zone_id: LOCAL_ZONE_ID,
        position: [x, ground + PLAYER_HALF_EXTENTS_UNITS[1], z],
        facing,
    };
    restore_health(&mut record);
    Ok(record)
}

/// Full health for the record's level and equipment.
pub(crate) fn restore_health(record: &mut CharacterRecord) {
    record.health = player_max_health(record.level) + record.equipment.totals().bonus_health();
}
