//! Which imported model presents a unit, its clip names and the per-unit clip clock.
//!
//! Players use asset-tooling's Ranger humanoid (Atlas 6.3.7); wolves, vermin and boars use the
//! bundled Quaternius CC0 animals (Atlas 6.3.6); the named Greyhaven residents wear the Ranger in
//! their appearance presets (#418) and the humanoid enemies (marauders, Redbrands) game-authored
//! presets (#39, `presets`) where the client draws them; other humanoid creatures and NPCs keep their
//! procedural models. Each client imports and skins the models with its own
//! provider; this module only decides what to pose. Presentation only, never read by gameplay.
use mmorpg_core::{
    AbilityId, CreatureFamily, CreatureTemplateId, EntityRef, EntitySnapshot, Equipment,
    EquipmentSlot, Ledge, PlayerClass, TraversalView, WaterMode, ZoneContent,
};
use strum::VariantArray;

use std::f64::consts::FRAC_PI_2;

use super::swim::Attitude;
use super::traversal::Motions;

/// What an imported humanoid carries in its sockets: the class weapons and props (#156, #244,
/// #302, asset-tooling #375 and #379) and the guards' sword.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Loadout {
    /// The Warden's arming sword and heater shield (#156).
    SwordAndShield,
    /// The Ranger's longbow and quiver on its back (#244).
    BowAndQuiver,
    /// The arming sword alone: the Greyhaven guards.
    Sword,
    /// The Rogue's blackened dagger.
    Dagger,
    /// The Necromancer's bone staff.
    BoneStaff,
    /// The Arcanist's crystal staff in the right hand and tome in the left (asset-tooling #379).
    CrystalStaffAndTome,
    /// The Cleric's flanged mace (asset-tooling #379).
    FlangedMace,
    /// The Druid's gnarled staff.
    GnarledStaff,
}

impl Loadout {
    /// The weapons and props a class carries.
    #[must_use]
    pub const fn of_class(class: PlayerClass) -> Self {
        match class {
            PlayerClass::Warden => Self::SwordAndShield,
            PlayerClass::Ranger => Self::BowAndQuiver,
            PlayerClass::Arcanist => Self::CrystalStaffAndTome,
            PlayerClass::Rogue => Self::Dagger,
            PlayerClass::Necromancer => Self::BoneStaff,
            PlayerClass::Cleric => Self::FlangedMace,
            PlayerClass::Druid => Self::GnarledStaff,
        }
    }

    /// The name both clients key their socket items by.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::SwordAndShield => "sword-and-shield",
            Self::BowAndQuiver => "bow-and-quiver",
            Self::Sword => "sword",
            Self::Dagger => "dagger",
            Self::BoneStaff => "staff.bone",
            Self::CrystalStaffAndTome => "staff.crystal-and-tome",
            Self::FlangedMace => "mace.flanged",
            Self::GnarledStaff => "staff.gnarled",
        }
    }
}

/// The armour slots an equipped drop covers, and the `humanoid-sockets` armour sockets whose
/// class outfit pieces it replaces. Hands, forearms and the belt belong to no slot and stay.
pub const ARMOR_SOCKETS: [(EquipmentSlot, &[&str]); 4] = [
    (EquipmentSlot::Head, &["head.armor"]),
    (
        EquipmentSlot::Chest,
        &[
            "spine_03.armor",
            "clavicle_l.armor",
            "clavicle_r.armor",
            "upperarm_l.armor",
            "upperarm_r.armor",
        ],
    ),
    (
        EquipmentSlot::Legs,
        &[
            "thigh_l.armor",
            "thigh_r.armor",
            "calf_l.armor",
            "calf_r.armor",
        ],
    ),
    (EquipmentSlot::Feet, &["foot_l.armor", "foot_r.armor"]),
];

/// The bundle outfit whose armour pieces draw an equipped drop. Catalog items carry no art of
/// their own, so every drop wears the knight's plate on the sockets its slot covers.
pub const DROP_ARMOR_OUTFIT: &str = "outfit.knight";

/// A player's class starter outfit (asset-tooling #379): the Ranger bundle's `class.<id>`
/// preset, its `classTrim` in the class colour, and the armour slots an equipped drop covers.
/// Drops replace the outfit's pieces on their sockets; the trimmed tabard, sash or robe stays,
/// so the class still reads.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Outfit {
    pub class: PlayerClass,
    /// Which of [`ARMOR_SOCKETS`] hold an equipped item. Only the viewer's equipment is
    /// projected, so everyone else wears the starter outfit whole.
    pub worn: [bool; 4],
}

impl Outfit {
    /// The class's starter outfit, `equipment` (the viewer's) covering its slots.
    #[must_use]
    pub fn new(class: PlayerClass, equipment: Option<&Equipment>) -> Self {
        Self {
            class,
            worn: ARMOR_SOCKETS
                .map(|(slot, _)| equipment.is_some_and(|equipment| equipment.item(slot).is_some())),
        }
    }

    /// The bundle's `humanoid-appearance.presets` id.
    #[must_use]
    pub const fn preset(self) -> &'static str {
        match self.class {
            PlayerClass::Warden => "class.warden",
            PlayerClass::Ranger => "class.ranger",
            PlayerClass::Arcanist => "class.arcanist",
            PlayerClass::Necromancer => "class.necromancer",
            PlayerClass::Rogue => "class.rogue",
            PlayerClass::Cleric => "class.cleric",
            PlayerClass::Druid => "class.druid",
        }
    }

    /// The armour sockets whose outfit pieces the equipped drops replace.
    #[must_use]
    pub fn replaced(self) -> Vec<&'static str> {
        ARMOR_SOCKETS
            .iter()
            .zip(self.worn)
            .filter(|(_, worn)| *worn)
            .flat_map(|((_, sockets), _)| sockets.iter().copied())
            .collect()
    }

    /// The class colour on the outfit's `classTrim` materials as a linear base colour: the
    /// class accent ([`super::player::Look::shoulder`], `shoulderColor` in the browser's
    /// `character-visuals.ts`).
    #[must_use]
    pub fn trim(self) -> [f32; 3] {
        let hex = super::player::Look::new(self.class.code(), false, None, false).shoulder;
        let linear = |byte: u32| {
            let c = (byte & 0xff) as f32 / 255.;
            if c <= 0.040_45 {
                c / 12.92
            } else {
                ((c + 0.055) / 1.055).powf(2.4)
            }
        };
        [linear(hex >> 16), linear(hex >> 8), linear(hex)]
    }
}

/// The Ranger figure a character is drawn on. Each is its own asset-tooling bundle with its own
/// skeleton, clips, sockets and `humanoid-appearance`: the male carries the players, the class
/// outfits and the male residents and enemies; the female (asset-tooling #378) carries only her
/// own presets, with no headwear, beards, head skins or variety options.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, strum::VariantArray)]
pub enum Figure {
    Male,
    Female,
}

impl Figure {
    /// The bundle's character key, also its acquisition directory under `characters/`.
    #[must_use]
    pub const fn character(self) -> &'static str {
        match self {
            Self::Male => "humanoid.ranger.male",
            Self::Female => "humanoid.ranger.female",
        }
    }

    /// The figure whose character key is `character`.
    #[must_use]
    pub fn of_character(character: &str) -> Option<Self> {
        Self::VARIANTS
            .iter()
            .copied()
            .find(|figure| figure.character() == character)
    }
}

/// A character drawn on a Ranger figure in a `humanoid-appearance` preset: a named Greyhaven
/// resident in the bundle's own preset (#418, #493), or a humanoid enemy in a game-authored
/// preset over the male bundle's outfits (#39; [`super::presets::enemy_presets`]).
#[derive(Clone, Copy, Debug, Eq, PartialEq, strum::VariantArray)]
pub enum Resident {
    Marshal,
    Guard,
    Innkeeper,
    Farmer,
    Healer,
    /// Tanner Hilda Brook (#493), on the female figure.
    Tanner,
    /// The Millbrook field marauders: ragged farmhands turned raiders.
    Marauder,
    /// The Redbrand rank and file in the band's red.
    RedbrandBandit,
    /// The Greyfang outriders (#286), a bow standing in for their crossbows.
    RedbrandCrossbowman,
    /// The Greyfang cavern's shield-bearers (#160): plate, helm, sword and shield.
    RedbrandBrute,
    /// Garrick Redbrand (#276), the band's bareheaded leader in plate.
    Garrick,
}

impl Resident {
    /// The resident an NPC of this canonical name is, if it has a preset.
    #[must_use]
    pub fn of_name(name: &str) -> Option<Self> {
        match name {
            "Marshal Elden Greywatch" => Some(Self::Marshal),
            "Greyhaven Guard" => Some(Self::Guard),
            "Innkeeper Bram Tolliver" => Some(Self::Innkeeper),
            "Farmer Osric Mill" => Some(Self::Farmer),
            "Brother Aldous" => Some(Self::Healer),
            "Tanner Hilda Brook" => Some(Self::Tanner),
            _ => None,
        }
    }

    /// The humanoid enemy a creature template of this name is, if it has a preset.
    #[must_use]
    pub fn of_creature(name: &str) -> Option<Self> {
        match name {
            "Field Marauder" => Some(Self::Marauder),
            "Redbrand Bandit" => Some(Self::RedbrandBandit),
            "Redbrand Crossbowman" => Some(Self::RedbrandCrossbowman),
            "Redbrand Brute" => Some(Self::RedbrandBrute),
            "Garrick Redbrand" => Some(Self::Garrick),
            _ => None,
        }
    }

    /// The `humanoid-appearance.presets` id: the bundle's for a resident, the game's for an
    /// enemy.
    #[must_use]
    pub const fn preset(self) -> &'static str {
        match self {
            Self::Marshal => "resident.marshal",
            Self::Guard => "resident.guard",
            Self::Innkeeper => "resident.innkeeper",
            Self::Farmer => "resident.farmer",
            Self::Healer => "resident.healer",
            Self::Tanner => "resident.tanner",
            Self::Marauder => "enemy.marauder",
            Self::RedbrandBandit => "enemy.redbrand.bandit",
            Self::RedbrandCrossbowman => "enemy.redbrand.crossbowman",
            Self::RedbrandBrute => "enemy.redbrand.brute",
            Self::Garrick => "enemy.redbrand.garrick",
        }
    }

    /// Whether the units sharing this preset each wear their own fixed variation of it
    /// (asset-tooling #380): the guards and the rank-and-file enemies are different men in the
    /// same gear, while the named residents and Garrick keep their authored looks.
    #[must_use]
    pub const fn varied(self) -> bool {
        matches!(
            self,
            Self::Guard
                | Self::Marauder
                | Self::RedbrandBandit
                | Self::RedbrandCrossbowman
                | Self::RedbrandBrute
        )
    }

    /// What the character carries: the guards, bandits and Garrick a sword, the brute a sword
    /// and shield, a crossbowman a bow and quiver, a marauder a dagger.
    #[must_use]
    pub const fn loadout(self) -> Option<Loadout> {
        match self {
            Self::Guard | Self::RedbrandBandit | Self::Garrick => Some(Loadout::Sword),
            Self::Marauder => Some(Loadout::Dagger),
            Self::RedbrandCrossbowman => Some(Loadout::BowAndQuiver),
            Self::RedbrandBrute => Some(Loadout::SwordAndShield),
            Self::Marshal | Self::Innkeeper | Self::Farmer | Self::Healer | Self::Tanner => None,
        }
    }

    /// The figure the character is drawn on: Hilda on the female, everyone else on the male.
    #[must_use]
    pub const fn figure(self) -> Figure {
        match self {
            Self::Tanner => Figure::Female,
            _ => Figure::Male,
        }
    }

    /// The class whose authored strike the character plays ([`Kind::strike`]): a crossbowman
    /// looses with the bow release; everyone else swings.
    #[must_use]
    pub const fn strike_class(self) -> Option<PlayerClass> {
        match self {
            Self::RedbrandCrossbowman => Some(PlayerClass::Ranger),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, strum::VariantArray)]
pub enum Kind {
    Ranger,
    Wolf,
    Rat,
    Pig,
}

/// Clip names of one model.
#[derive(Clone, Copy, Debug)]
pub struct Clips {
    pub idle: &'static str,
    pub walk: &'static str,
    pub run: &'static str,
    pub attack: &'static str,
    pub death: &'static str,
    /// A standing raised shield, for a model that carries one (#156).
    pub guard: Option<&'static str>,
    pub jump: Option<JumpClips>,
    /// The authored water locomotion, for a model that swims (Atlas 6.6.41).
    pub swim: Option<SwimClips>,
    /// The in-place dodge roll, for a model that rolls (#516).
    pub roll: Option<&'static str>,
    /// Authored climbing, ladder, rope and mantle clips, for a model that climbs (#500).
    pub climb: Option<ClimbClips>,
    /// Class- and ability-specific strikes, for a model that has them (#384).
    pub strikes: Option<StrikeClips>,
    /// The joint whose subtree is the upper body, for a model that layers its upper body over
    /// its legs (Atlas 6.6.45); every other joint (root, pelvis, legs) is the lower body.
    pub upper_body: Option<&'static str>,
    /// Directional, sprint and turning gaits, for a model that has them (#538).
    pub gaits: Option<GaitClips>,
    /// Hit, block and guard-break reactions, for a model that has them (#538).
    pub reactions: Option<ReactionClips>,
}

/// The authored strikes a humanoid plays instead of its sword attack.
#[derive(Clone, Copy, Debug)]
pub struct StrikeClips {
    /// The Warden's Shield Bash, the shield-arm follow-up of the slash combo.
    pub shield_bash: &'static str,
    /// A bow shot's release.
    pub bow: &'static str,
    /// A spell's release, for the staff and spell classes.
    pub spell: &'static str,
    /// The bow cycle around the release (#538): aimed while the shot winds up, drawn and held
    /// through a cast, recovered after the release and let down on a cancel.
    pub bow_cycle: CastClips,
    /// The spell hold (#538): raised, held through a cast or channel and lowered on a cancel.
    pub spell_cycle: CastClips,
}

/// A cast's clips (#538): `enter` once, then `hold` looped until the cast resolves or stops;
/// `recovery` after its release, `cancel` when it stops early. `aim` is the local wind-up of an
/// instant release.
#[derive(Clone, Copy, Debug)]
pub struct CastClips {
    pub aim: Option<&'static str>,
    pub enter: &'static str,
    pub hold: &'static str,
    pub recovery: Option<&'static str>,
    pub cancel: &'static str,
    /// The cycle turns the pelvis: the whole body plays it while standing, the upper body
    /// while moving. Otherwise only the upper body plays it.
    pub footwork: bool,
}

impl CastClips {
    /// Whether `clip` is one of the cycle's clips.
    #[must_use]
    pub fn contains(self, clip: &str) -> bool {
        [
            Some(self.enter),
            Some(self.hold),
            Some(self.cancel),
            self.aim,
            self.recovery,
        ]
        .contains(&Some(clip))
    }
}

/// Where the Ranger's longbow rides (#556): stowed on the back, and in the left hand through the
/// bow cycle.
pub const BOW_STOWED_SOCKET: &str = "back.bow.stowed";
pub const BOW_HAND_SOCKET: &str = "hand_l.bow";
/// The longbow's draw contract (asset-tooling `BOW_DRAW_CONTRACT`): its `nock` joint moves
/// along the bow's `-Z` by the clip's `humanoid-clips` `bow.drawMeters`.
pub const BOW_NOCK_JOINT: &str = "nock";
pub const BOW_DRAW_AXIS: [f32; 3] = [0., 0., -1.];

/// The bow cycle clip a unit's upper body shows (#556): while it plays, the bow is in hand and
/// its nock follows the clip's `bow.drawMeters` at `time`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BowShot {
    pub clip: &'static str,
    pub time: f32,
}

/// The draw in metres at `time` seconds of a clip's `bow.drawMeters` (one value per frame at
/// `sample_rate`), linear between frames and held at either end; 0 for an empty table.
#[must_use]
pub fn draw_meters(table: &[f32], sample_rate: f32, time: f32) -> f32 {
    let Some(last) = table.len().checked_sub(1) else {
        return 0.;
    };
    #[allow(clippy::cast_precision_loss)]
    let frame = (time * sample_rate).clamp(0., last as f32);
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let below = (frame.floor() as usize).min(last);
    let above = (below + 1).min(last);
    #[allow(clippy::cast_precision_loss)]
    let share = frame - below as f32;
    table[below] + (table[above] - table[below]) * share
}

/// In-place directional, sprint and turning locomotion (#538). A gait the bundle lacks falls
/// back to the forward walk or run.
#[derive(Clone, Copy, Debug)]
pub struct GaitClips {
    pub walk_back: &'static str,
    pub walk_left: &'static str,
    pub walk_right: &'static str,
    pub run_back: &'static str,
    pub run_left: &'static str,
    pub run_right: &'static str,
    pub sprint: &'static str,
    pub turn_left: &'static str,
    pub turn_right: &'static str,
}

/// The reactions to blows (#538), by [`Reaction`].
#[derive(Clone, Copy, Debug)]
pub struct ReactionClips {
    pub chest: &'static str,
    pub head: &'static str,
    pub knockback: &'static str,
    pub shove: &'static str,
    pub block: &'static str,
    pub guard_break: &'static str,
}

impl ReactionClips {
    #[must_use]
    pub const fn clip(self, reaction: Reaction) -> &'static str {
        match reaction {
            Reaction::Chest => self.chest,
            Reaction::Head => self.head,
            Reaction::Knockback => self.knockback,
            Reaction::Shove => self.shove,
            Reaction::Block => self.block,
            Reaction::GuardBreak => self.guard_break,
        }
    }
}

/// How a unit reacts to a blow the zone resolved against it (#538). Presentation only.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Reaction {
    /// An ordinary hit.
    Chest,
    /// A critical hit.
    Head,
    /// A forced-move knockback (#287): a bash, blast or charge.
    Knockback,
    /// A large creature's stomp or smash (#297, #160).
    Shove,
    /// A blow its held guard blocked (#156).
    Block,
    /// A blow that broke its guard (#156).
    GuardBreak,
}

impl Reaction {
    /// Which body the reaction drives: a hit flinches the whole body standing and the upper
    /// body on the move; a knockback or shove always throws the whole body; a block or guard
    /// break is the shield arm's.
    const fn region(self) -> Region {
        match self {
            Self::Chest | Self::Head => Region::Footwork,
            Self::Knockback | Self::Shove => Region::Whole,
            Self::Block | Self::GuardBreak => Region::Upper,
        }
    }

    /// An ordinary hit never cuts the unit's own strike short.
    const fn interrupts(self) -> bool {
        !matches!(self, Self::Chest | Self::Head)
    }
}

/// Which body regions a one-shot drives.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Region {
    /// The whole body while standing on the ground, the upper body once it moves (Atlas 6.6.2).
    Footwork,
    /// Always the whole body.
    Whole,
    /// Only the upper body.
    Upper,
}

/// The Ranger skeleton's upper-body root: spine, arms and head move under it.
pub const UPPER_BODY_ROOT: &str = "spine_01";

/// In-place authored traversal; these clips never move the presentation root.
#[derive(Clone, Copy, Debug)]
pub struct JumpClips {
    pub start: &'static str,
    pub airborne: &'static str,
    pub land: &'static str,
}

/// In-place authored water locomotion (Atlas 6.6.41), shown in the zone's water mode. A clip the
/// bundle lacks falls back to the authored land clip nearest to it: the walk for wading and
/// strokes (leaned into the stroke by [`Attitude`]), the idle for treading, and no transition at
/// all for entry and exit.
#[derive(Clone, Copy, Debug)]
pub struct SwimClips {
    /// Walking through water up to the waist.
    pub wade: &'static str,
    /// Treading water in place, head above the surface or holding depth.
    pub tread: &'static str,
    /// Horizontal swimming, at the surface or submerged.
    pub stroke: &'static str,
    /// Swimming downward, a dive.
    pub descend: &'static str,
    /// Swimming upward, toward the surface.
    pub ascend: &'static str,
    /// Wading or falling into swimming depth.
    pub enter: &'static str,
    /// Leaving swimming depth for wading depth or the shore.
    pub exit: &'static str,
}

/// In-place authored traversal on climbable surfaces (#500, Atlas 6.6.43, asset-tooling #416),
/// shown in the zone's projected climb or mantle. A clip the bundle lacks falls back in place to
/// the land clips, as before the clips existed: the idle holding on and the walk climbing.
#[derive(Clone, Copy, Debug)]
pub struct ClimbClips {
    /// Holding a rock or timber face still, and climbing it up, down, left and right.
    pub idle: &'static str,
    pub up: &'static str,
    pub down: &'static str,
    pub left: &'static str,
    pub right: &'static str,
    /// Pulling up over the top of a face being climbed.
    pub top_out: &'static str,
    /// Letting go of a face, or dropping off it.
    pub let_go: &'static str,
    pub ladder_idle: &'static str,
    pub ladder_up: &'static str,
    pub ladder_down: &'static str,
    pub rope_idle: &'static str,
    pub rope_up: &'static str,
    pub rope_down: &'static str,
    /// Mantling a low (0.2–1 m), mid (1–1.6 m) and high (1.6–2.2 m) ledge from the ground.
    pub mantle_low: &'static str,
    pub mantle_mid: &'static str,
    pub mantle_high: &'static str,
}

impl ClimbClips {
    /// The mantle of a `ledge` from the ground.
    #[must_use]
    pub const fn mantle(self, ledge: Ledge) -> &'static str {
        match ledge {
            Ledge::Low => self.mantle_low,
            Ledge::Mid => self.mantle_mid,
            Ledge::High => self.mantle_high,
        }
    }
}

/// Climbing travel along the face below this speed in metres per second holds on still.
pub const CLIMB_MOVING_SPEED: f64 = 0.15;
/// The speeds in metres per second at which a climb loop plays at its authored rate when the
/// bundle does not say (`climbSpeedMetersPerSecond`): the zone's climb up or down and sideways,
/// and the clips' ladder and rope step.
pub const CLIMB_CLIP_SPEED: f64 = 0.9;
pub const CLIMB_SIDE_CLIP_SPEED: f64 = 0.75;
pub const LADDER_CLIP_SPEED: f64 = 0.6;
/// The zone's mantle rise and move across onto the ledge, in metres per second, and how far it
/// moves across (`mmorpg_core::climb`): a mantle clip plays fast enough to end about when the
/// zone's mantle does.
const MANTLE_RISE_SPEED: f64 = mmorpg_core::climb::MANTLE_RISE_UNITS_PER_SECOND as f64 / 100.;
const MANTLE_ACROSS_SPEED: f64 = mmorpg_core::climb::MANTLE_FORWARD_UNITS_PER_SECOND as f64 / 100.;
const MANTLE_ACROSS_METERS: f64 = 0.7;
/// The fastest a mantle clip plays.
pub const MANTLE_MAX_RATE: f32 = 1.6;

/// The stroke speed in metres per second at which a swim clip plays at its authored rate.
pub const STROKE_SPEED: f64 = 1.8;
/// Ground speed in metres per second above which forward or sideways travel plays the run clip:
/// the player's 6.3 m/s run does, while slowed movement walks (#515).
pub const RUN_CLIP_SPEED: f64 = 3.2;
/// Vertical speed in metres per second from which a swimmer dives or ascends.
pub const DIVE_SPEED: f64 = 0.4;
/// The shortest lower-body crossfade in seconds just after entering or leaving swimming depth.
pub const WATER_BLEND_SECONDS: f32 = 0.3;
/// The zone's dodge roll lasts this long in seconds (#516); its clip plays once over it.
pub const ROLL_SECONDS: f64 = mmorpg_core::roll::ROLL_TICKS as f64 / mmorpg_core::TICK_HZ as f64;
/// Ground speed in metres per second at which the forward run, and the run-speed strafes and
/// backpedal, play at their authored rate.
pub const RUN_CLIP_GROUND: f64 = 5.;
/// Ground speed in metres per second above which forward travel sprints (#538): above the
/// player's 6.3 m/s run, so only a faster mover (a charge) sprints.
pub const SPRINT_CLIP_SPEED: f64 = 7.5;
/// Ground speed in metres per second at which the sprint plays at its authored rate: the run's,
/// scaled by the clips' `groundSpeedMetersPerSecond` (2.13 m/s sprint, 1.76 m/s jog).
pub const SPRINT_CLIP_GROUND: f64 = RUN_CLIP_GROUND * 2.1345 / 1.7637;
/// Travel within this angle of the facing, in radians, plays the forward gait; beyond
/// [`BACKPEDAL_ANGLE`] the backward one; in between the strafes (#538). A diagonal stays
/// forward or backward, so it never flickers between gaits.
pub const STRAFE_ANGLE: f64 = std::f64::consts::FRAC_PI_3;
pub const BACKPEDAL_ANGLE: f64 = 2. * std::f64::consts::FRAC_PI_3;
/// A standing unit turning faster than this, in radians per second, steps round with the turn
/// clips (#538), and keeps stepping [`TURN_HOLD_SECONDS`] after it slows.
pub const TURN_CLIP_RATE: f64 = 1.5;
pub const TURN_HOLD_SECONDS: f64 = 0.2;
/// The turn clips' authored quarter turn plays at its rate at this many radians per clip.
const TURN_CLIP_RADIANS: f64 = FRAC_PI_2;
/// The longest a reaction plays, in seconds: a long shove spin stops here and blends out.
pub const REACTION_SECONDS: f32 = 1.5;

impl Kind {
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Ranger => "Ranger",
            Self::Wolf => "Wolf",
            Self::Rat => "Rat",
            Self::Pig => "Pig",
        }
    }

    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        Self::VARIANTS
            .iter()
            .copied()
            .find(|kind| kind.name() == name)
    }

    #[must_use]
    pub const fn clips(self) -> Clips {
        match self {
            Self::Ranger => Clips {
                idle: "Idle_Loop",
                walk: "Walk_Loop",
                run: "Jog_Fwd_Loop",
                attack: "Sword_Attack",
                death: "Death01",
                guard: Some("Idle_Shield_Loop"),
                jump: Some(JumpClips {
                    start: "Jump_Start",
                    airborne: "Jump_Loop",
                    land: "Jump_Land",
                }),
                swim: Some(SwimClips {
                    wade: "Wade_Fwd_Loop",
                    tread: "Swim_Idle_Loop",
                    stroke: "Swim_Fwd_Loop",
                    descend: "Swim_Down_Loop",
                    ascend: "Swim_Up_Loop",
                    enter: "Swim_Enter",
                    exit: "Swim_Exit",
                }),
                roll: Some("Roll"),
                climb: Some(ClimbClips {
                    idle: "Climb_Idle_Loop",
                    up: "Climb_Up_Loop",
                    down: "Climb_Down_Loop",
                    left: "Climb_Left_Loop",
                    right: "Climb_Right_Loop",
                    top_out: "Climb_TopOut",
                    let_go: "Climb_LetGo",
                    ladder_idle: "Ladder_Idle_Loop",
                    ladder_up: "Ladder_Up_Loop",
                    ladder_down: "Ladder_Down_Loop",
                    rope_idle: "Rope_Idle_Loop",
                    rope_up: "Rope_Up_Loop",
                    rope_down: "Rope_Down_Loop",
                    mantle_low: "Mantle_Low",
                    mantle_mid: "Mantle_Mid",
                    mantle_high: "Mantle_High",
                }),
                strikes: Some(StrikeClips {
                    shield_bash: "Shield_OneShot",
                    bow: "Bow_Release",
                    spell: "Spell_Simple_Shoot",
                    bow_cycle: CastClips {
                        aim: Some("Bow_Aim_Loop"),
                        enter: "Bow_Draw",
                        hold: "Bow_Hold_Loop",
                        recovery: Some("Bow_Recovery"),
                        cancel: "Bow_Cancel",
                        footwork: true,
                    },
                    spell_cycle: CastClips {
                        aim: None,
                        enter: "Spell_Simple_Enter",
                        hold: "Spell_Simple_Idle_Loop",
                        recovery: None,
                        cancel: "Spell_Simple_Exit",
                        footwork: false,
                    },
                }),
                upper_body: Some(UPPER_BODY_ROOT),
                gaits: Some(GaitClips {
                    walk_back: "Walk_Bwd_Loop",
                    walk_left: "Walk_Strafe_Left_Loop",
                    walk_right: "Walk_Strafe_Right_Loop",
                    run_back: "Jog_Bwd_Loop",
                    run_left: "Jog_Strafe_Left_Loop",
                    run_right: "Jog_Strafe_Right_Loop",
                    sprint: "Sprint_Loop",
                    turn_left: "Turn_Left",
                    turn_right: "Turn_Right",
                }),
                reactions: Some(ReactionClips {
                    chest: "Hit_Chest",
                    head: "Hit_Head",
                    knockback: "Hit_Knockback",
                    shove: "Hit_Shove_Spin",
                    block: "Sword_Block",
                    guard_break: "Idle_Shield_Break",
                }),
            },
            Self::Wolf => Clips {
                idle: "Idle",
                walk: "Walk",
                run: "Gallop",
                attack: "Attack",
                death: "Death",
                guard: None,
                jump: None,
                swim: None,
                roll: None,
                climb: None,
                strikes: None,
                upper_body: None,
                gaits: None,
                reactions: None,
            },
            Self::Rat => Clips {
                idle: "Rat_Idle",
                walk: "Rat_Walk",
                run: "Rat_Run",
                attack: "Rat_Attack",
                death: "Rat_Death",
                guard: None,
                jump: None,
                swim: None,
                roll: None,
                climb: None,
                strikes: None,
                upper_body: None,
                gaits: None,
                reactions: None,
            },
            // The farm pig has no attack clip; its jump reads as a charge.
            Self::Pig => Clips {
                idle: "Idle",
                walk: "Walk",
                run: "Run",
                attack: "Jump",
                death: "Death",
                guard: None,
                jump: None,
                swim: None,
                roll: None,
                climb: None,
                strikes: None,
                upper_body: None,
                gaits: None,
                reactions: None,
            },
        }
    }

    /// The authored strike an accepted attack or ability of a `class` user plays: the Warden's
    /// Shield Bash its shield-arm clip, a bow class its bow release, a staff or spell class its
    /// spell release, everyone else (and a model without them) its attack.
    #[must_use]
    pub fn strike(self, class: Option<PlayerClass>, ability: Option<AbilityId>) -> &'static str {
        let clips = self.clips();
        let Some(strikes) = clips.strikes else {
            return clips.attack;
        };
        if ability == Some(mmorpg_core::ability::ids::SHIELD_BASH) {
            return strikes.shield_bash;
        }
        match class {
            Some(PlayerClass::Ranger) => strikes.bow,
            Some(
                PlayerClass::Arcanist
                | PlayerClass::Necromancer
                | PlayerClass::Cleric
                | PlayerClass::Druid,
            ) => strikes.spell,
            Some(PlayerClass::Warden | PlayerClass::Rogue) | None => clips.attack,
        }
    }

    /// The cast cycle a `class` user shows while casting or channelling (#538): a bow class
    /// draws and holds the bow, everyone else holds a spell.
    #[must_use]
    pub fn cast(self, class: Option<PlayerClass>) -> Option<CastClips> {
        let strikes = self.clips().strikes?;
        Some(if class == Some(PlayerClass::Ranger) {
            strikes.bow_cycle
        } else {
            strikes.spell_cycle
        })
    }

    /// The cast cycle whose release `strike` is, for its wind-up and recovery (#538).
    #[must_use]
    pub fn cycle_of(self, strike: &str) -> Option<CastClips> {
        let strikes = self.clips().strikes?;
        if strike == strikes.bow {
            Some(strikes.bow_cycle)
        } else if strike == strikes.spell {
            Some(strikes.spell_cycle)
        } else {
            None
        }
    }

    /// The bow shot `animation` shows (#556): its upper-body target is the bow release or a
    /// clip of the bow cycle.
    #[must_use]
    pub fn bow_shot(self, animation: &Animation) -> Option<BowShot> {
        let strikes = self.clips().strikes?;
        let (clip, time) = animation.upper;
        (clip == strikes.bow || strikes.bow_cycle.contains(clip)).then_some(BowShot { clip, time })
    }

    /// The clip of a `reaction`, for a model that has reactions.
    #[must_use]
    pub fn reaction(self, reaction: Reaction) -> Option<&'static str> {
        self.clips()
            .reactions
            .map(|reactions| reactions.clip(reaction))
    }

    /// The model presenting `entity`, if it has one.
    #[must_use]
    pub fn of(entity: EntityRef, record: &EntitySnapshot, content: &ZoneContent) -> Option<Self> {
        match entity {
            EntityRef::Player(_) => Some(Self::Ranger),
            EntityRef::Creature(_) => Self::of_family(
                content
                    .creature_template(CreatureTemplateId::new(record.appearance))?
                    .family,
            ),
            // A Druid's companion (#296) borrows the CC0 Quaternius wolf as its placeholder;
            // skeletons (#295) are procedural placeholders until their art exists.
            EntityRef::Minion(_) => {
                mmorpg_core::is_companion(mmorpg_core::minion_ability(record.appearance))
                    .then_some(Self::Wolf)
            }
            EntityRef::Npc(_) => None,
        }
    }

    /// The model creatures of `family` use, if any.
    #[must_use]
    pub const fn of_family(family: CreatureFamily) -> Option<Self> {
        match family {
            CreatureFamily::Wolf => Some(Self::Wolf),
            CreatureFamily::Vermin => Some(Self::Rat),
            CreatureFamily::Boar => Some(Self::Pig),
            CreatureFamily::Marauder
            | CreatureFamily::Mirefin
            | CreatureFamily::Redbrand
            | CreatureFamily::Ogre
            | CreatureFamily::Prop => None,
        }
    }
}

/// Which joints a clip layer drives (Atlas 6.6.45).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Mask {
    /// Every joint.
    All,
    /// The [`Clips::upper_body`] subtree: spine, arms and head.
    Upper,
    /// Every other joint: root, pelvis and legs.
    Lower,
}

impl Mask {
    /// The name both clients and the Action Lab read.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::All => "all",
            Self::Upper => "upper",
            Self::Lower => "lower",
        }
    }

    /// Whether a layer with this mask drives a joint that is (or is not) in the upper body.
    #[must_use]
    pub const fn drives(self, upper: bool) -> bool {
        match self {
            Self::All => true,
            Self::Upper => upper,
            Self::Lower => !upper,
        }
    }
}

/// One weighted clip sample. A joint takes the weighted average of the layers that drive and
/// animate it; when their weights sum below 1 the remainder stays at the rest pose.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Layer {
    pub clip: &'static str,
    pub time: f32,
    pub weight: f32,
    pub mask: Mask,
}

impl Layer {
    /// One clip on the whole body.
    #[must_use]
    pub const fn single(clip: &'static str, time: f32) -> Self {
        Self {
            clip,
            time,
            weight: 1.,
            mask: Mask::All,
        }
    }
}

/// How the body regions blend (Atlas 6.6.45). The Action Lab tunes it (#384); every other view
/// uses the default. Presentation only: no value here changes what the zone accepts.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Blend {
    /// Seconds a lower-body (pelvis and legs) change crossfades over.
    pub lower_seconds: f32,
    /// Seconds an upper-body strike or guard blends in or out over.
    pub upper_seconds: f32,
    /// How strongly a landing or a grounded whole-body strike drives the legs over locomotion.
    pub lower_weight: f32,
    /// How strongly a strike or guard drives the upper body over the legs' clip.
    pub upper_weight: f32,
}

impl Default for Blend {
    fn default() -> Self {
        Self {
            lower_seconds: 0.15,
            upper_seconds: 0.1,
            lower_weight: 1.,
            upper_weight: 1.,
        }
    }
}

impl Blend {
    /// Durations within 0–1 s and weights within 0–1; a non-finite value takes the default.
    #[must_use]
    pub fn clamped(self) -> Self {
        let fallback = Self::default();
        let clamp = |value: f32, fallback: f32| {
            if value.is_finite() {
                value.clamp(0., 1.)
            } else {
                fallback
            }
        };
        Self {
            lower_seconds: clamp(self.lower_seconds, fallback.lower_seconds),
            upper_seconds: clamp(self.upper_seconds, fallback.upper_seconds),
            lower_weight: clamp(self.lower_weight, fallback.lower_weight),
            upper_weight: clamp(self.upper_weight, fallback.upper_weight),
        }
    }
}

/// What one unit shows this frame: the layers to pose and the clip each region blends toward.
#[derive(Clone, Debug, PartialEq)]
pub struct Animation {
    pub layers: Vec<Layer>,
    /// The pelvis and legs' target clip and time.
    pub lower: (&'static str, f32),
    /// The upper body's target clip and time; a model without layers shows it whole.
    pub upper: (&'static str, f32),
    /// The body's cosmetic swim lean and turn about its chest (Atlas 6.6.41); upright out of
    /// swimming depth.
    pub attitude: Attitude,
}

/// What the projection shows of one unit this frame. Every cue is zone-accepted state or the
/// projected motion; the clip clocks never feed back into it.
#[derive(Clone, Copy, Debug, Default)]
pub struct Cues<'a> {
    /// The zone's projected climb, mantle or roll (#490, #500, #516).
    pub traversal: TraversalView,
    /// The model's `humanoid-clips` traversal data, once loaded (#500).
    pub motions: Option<&'a Motions>,
    /// Horizontal speed in metres per second.
    pub speed: f64,
    /// The horizontal travel's direction from the unit's facing, as a yaw offset in radians.
    pub direction: f64,
    /// How fast the facing turns, in radians per second; positive turns left (#538).
    pub turn: f64,
    /// Cosmetic vertical velocity in metres per second.
    pub vertical_speed: f64,
    /// The zone's water mode at the unit's projected position (Atlas 6.6.41).
    pub water: WaterMode,
    pub dead: bool,
    /// The zone's guard protects now: a ready, grounded raised shield (viewer only).
    pub guarding: bool,
    /// The zone projects a dodge roll (#516).
    pub rolling: bool,
    /// Presentation clock and the step since the previous frame, in seconds.
    pub now: f64,
    pub dt: f64,
    pub reduced: bool,
}

/// One clip a track plays. `serial` tells a repeated strike from the one before it.
#[derive(Clone, Copy, Debug)]
struct Play {
    clip: &'static str,
    serial: u32,
    rate: f32,
    once: bool,
    time: f32,
}

/// The clip a track should play: a loop at `rate`, or a one-shot held on its last frame. A
/// strike's `at` pins its time to the strike clock so every region swings in step.
#[derive(Clone, Copy, Debug)]
struct Want {
    clip: &'static str,
    serial: u32,
    rate: f32,
    once: bool,
    at: Option<f32>,
}

impl Want {
    const fn looped(clip: &'static str, rate: f32) -> Self {
        Self {
            clip,
            serial: 0,
            rate,
            once: false,
            at: None,
        }
    }

    const fn hold(clip: &'static str, serial: u32) -> Self {
        Self {
            clip,
            serial,
            rate: 1.,
            once: true,
            at: None,
        }
    }
}

/// A region's clip, crossfading from the one before it. Playing nothing lets the region
/// underneath show through, so a track also fades a whole layer in and out.
#[derive(Clone, Copy, Debug, Default)]
struct Track {
    current: Option<Play>,
    previous: Option<Play>,
    /// The current clip's share of the crossfade, 0–1.
    fade: f32,
}

impl Track {
    fn advance(&mut self, want: Option<Want>, dt: f32, seconds: f32, reduced: bool) {
        let step = |play: &mut Play| {
            if !reduced {
                play.time += dt * play.rate;
            }
        };
        if let Some(previous) = &mut self.previous {
            step(previous);
        }
        let same = match (&self.current, want) {
            (Some(current), Some(want)) => {
                current.clip == want.clip && current.serial == want.serial
            }
            (None, None) => true,
            _ => false,
        };
        let instant = reduced || seconds <= 0.;
        if same {
            if let (Some(current), Some(want)) = (&mut self.current, want) {
                current.rate = want.rate;
                step(current);
            }
            self.fade = if instant {
                1.
            } else {
                (self.fade + dt / seconds).min(1.)
            };
        } else {
            self.previous = self.current;
            self.current = want.map(|want| Play {
                clip: want.clip,
                serial: want.serial,
                rate: want.rate,
                once: want.once,
                time: 0.,
            });
            self.fade = if instant || self.previous.is_none() && self.current.is_none() {
                1.
            } else {
                0.
            };
        }
        if let (Some(current), Some(at)) = (&mut self.current, want.and_then(|want| want.at)) {
            current.time = at;
        }
        if self.fade >= 1. {
            self.previous = None;
        }
    }

    /// The current and fading clips as layers, their weights scaled by `scale`.
    fn layers(
        &self,
        duration: &impl Fn(&str) -> f32,
        scale: f32,
        mask: Mask,
        out: &mut Vec<Layer>,
    ) {
        for (play, share) in [(self.previous, 1. - self.fade), (self.current, self.fade)] {
            let weight = share * scale;
            if let Some(play) = play
                && weight > 1e-3
            {
                out.push(Layer {
                    clip: play.clip,
                    time: sample(play, duration),
                    weight,
                    mask,
                });
            }
        }
    }

    fn target(&self, duration: &impl Fn(&str) -> f32) -> Option<(&'static str, f32)> {
        self.current.map(|play| (play.clip, sample(play, duration)))
    }

    fn playing(&self, clip: &str) -> Option<Play> {
        self.current.filter(|play| play.clip == clip)
    }
}

/// The walk's rate at `speed` metres per second.
#[allow(clippy::cast_possible_truncation)]
fn walk_rate(speed: f64) -> f32 {
    (speed / 1.8).clamp(0.6, 1.6) as f32
}

fn sample(play: Play, duration: &impl Fn(&str) -> f32) -> f32 {
    let length = duration(play.clip);
    if length <= 0. {
        0.
    } else if play.once {
        play.time.min(length)
    } else {
        play.time % length
    }
}

/// An accepted strike, a reaction or a cast being presented.
#[derive(Clone, Copy, Debug)]
struct Strike {
    clip: &'static str,
    /// `clip`'s length in seconds; `then` follows it.
    length: f32,
    /// What plays after `clip`: a recovery once, or a cast's hold looped.
    then: Option<(&'static str, bool)>,
    serial: u32,
    started: f64,
    until: f64,
    region: Region,
    /// The reaction it shows, if it is one.
    reaction: Option<Reaction>,
    /// Begun standing on the ground: the authored footwork plays on the legs too (Atlas 6.6.2),
    /// until the unit moves or leaves the ground; then only the upper body carries it.
    full_body: bool,
}

impl Strike {
    fn new(clip: &'static str, length: f32, serial: u32, started: f64, region: Region) -> Self {
        Self {
            clip,
            length,
            then: None,
            serial,
            started,
            until: started + f64::from(length.max(0.3)),
            region,
            reaction: None,
            full_body: region != Region::Upper,
        }
    }

    /// The clip and pinned time to show at `now`.
    #[allow(clippy::cast_possible_truncation)]
    fn want(self, now: f64) -> Want {
        let at = (now - self.started).max(0.) as f32;
        match self.then {
            Some((next, looped)) if at >= self.length => Want {
                once: !looped,
                at: Some(at - self.length),
                ..Want::hold(next, self.serial)
            },
            _ => Want {
                at: Some(at),
                ..Want::hold(self.clip, self.serial)
            },
        }
    }
}

/// How far into its strike clip a local wind-up (Atlas 6.6.10) goes before it holds: this share
/// of the clip, at most [`WINDUP_HOLD_SECONDS`], so it stops short of the swing's impact.
pub const WINDUP_FRACTION: f32 = 0.3;
/// The longest a wind-up plays before holding, in seconds.
pub const WINDUP_HOLD_SECONDS: f32 = 0.3;

/// The local player's reversible wind-up (Atlas 6.6.10): the opening of the strike clip the
/// prepared input would play, on the upper body only and held before impact. The zone's
/// accepted strike continues it; a refusal or supersession lets it fade out.
#[derive(Clone, Copy, Debug)]
struct Preparation {
    clip: &'static str,
    serial: u32,
    started: f64,
}

impl Preparation {
    /// The wind-up's clip time at `now` for a clip `length` seconds long.
    #[allow(clippy::cast_possible_truncation)]
    fn at(self, now: f64, length: f32) -> f32 {
        let hold = (length * WINDUP_FRACTION).min(WINDUP_HOLD_SECONDS);
        ((now - self.started).max(0.) as f32).min(hold)
    }
}

/// Per-unit layered clip choice and clocks (Atlas 6.6.45). Presentation-owned and cosmetic:
/// the clips follow accepted strikes, the projected guard and the projected motion, and never
/// decide a hit, readiness, a window or the root.
#[derive(Clone, Debug, Default)]
pub struct Playback {
    flight: Flight,
    strike: Option<Strike>,
    /// A cast or channel in progress (#538), under any strike or reaction.
    cast: Option<Strike>,
    /// Stepping round with a turn clip until this time, turning left or right (#538).
    turning: Option<(bool, f64)>,
    /// The local player's wind-up, until the zone answers (Atlas 6.6.10).
    preparation: Option<Preparation>,
    guarding: bool,
    serial: u32,
    landing: u32,
    /// In swimming depth last frame (Atlas 6.6.41).
    swimming: bool,
    /// The water entry or exit being shown, and how many have begun.
    crossing: Option<&'static str>,
    crossings: u32,
    /// When the unit last entered or left swimming depth, on the presentation clock.
    crossed: Option<f64>,
    /// Rolling last frame (#516), and how many rolls have begun.
    rolling: bool,
    rolls: u32,
    /// The zone's climb or mantle last frame (#500).
    held: TraversalView,
    /// The mantle or top-out being shown and its rate, until it plays out (#500) …
    mantle: Option<(&'static str, f32)>,
    /// … the let-go being shown, and how many of either have begun.
    letting_go: Option<(&'static str, f64)>,
    pulls: u32,
    attitude: Attitude,
    /// Pelvis and legs: locomotion, flight and death.
    base: Track,
    /// Over the legs' locomotion: a landing or a grounded whole-body strike.
    lower: Track,
    /// Spine, arms and head over the legs' clip: a strike or the guard.
    upper: Track,
}

#[derive(Clone, Copy, Debug, Default)]
enum Flight {
    #[default]
    Ground,
    Rising,
    Airborne,
    Falling,
    Landing,
}

impl Playback {
    /// Starts presenting an accepted strike's `clip`, `duration` seconds long, at `now`.
    /// A strike resolves any cast shown before it.
    pub fn strike(&mut self, clip: &'static str, duration: f32, now: f64) {
        self.serial = self.serial.wrapping_add(1);
        self.strike = Some(Strike::new(
            clip,
            duration,
            self.serial,
            now,
            Region::Footwork,
        ));
        self.cast = None;
    }

    /// The zone accepted the prepared strike: it continues from the wind-up's phase when the
    /// wind-up shows the same clip, so the swing commits without restarting. Otherwise it is an
    /// ordinary [`Self::strike`] and the wind-up ends.
    pub fn commit(&mut self, clip: &'static str, duration: f32, now: f64) {
        match self.preparation.take() {
            Some(preparation) if preparation.clip == clip => {
                let started = now - f64::from(preparation.at(now, duration));
                self.strike = Some(Strike::new(
                    clip,
                    duration,
                    preparation.serial,
                    started,
                    Region::Footwork,
                ));
                self.cast = None;
            }
            _ => self.strike(clip, duration, now),
        }
    }

    /// The strike just begun recovers with `clip`, `duration` seconds long, after it (#538): a
    /// bow shot's recovery after its release.
    pub fn recover(&mut self, clip: &'static str, duration: f32) {
        if let Some(strike) = &mut self.strike
            && duration > 0.
        {
            strike.then = Some((clip, false));
            strike.until = strike.started + f64::from(strike.length.max(0.3) + duration);
        }
    }

    /// Reacts to a blow the zone resolved with `clip`, `duration` seconds long (#538), for at
    /// most [`REACTION_SECONDS`]. An ordinary hit never cuts the unit's own strike short.
    pub fn react(&mut self, reaction: Reaction, clip: &'static str, duration: f32, now: f64) {
        if !reaction.interrupts()
            && self
                .strike
                .is_some_and(|strike| strike.reaction.is_none() && now < strike.until)
        {
            return;
        }
        self.serial = self.serial.wrapping_add(1);
        let mut strike = Strike::new(
            clip,
            duration.min(REACTION_SECONDS),
            self.serial,
            now,
            reaction.region(),
        );
        strike.reaction = Some(reaction);
        self.strike = Some(strike);
    }

    /// Shows a cast or channel of `seconds` begun at `now` (#538): `enter` once, when the model
    /// has it, then `hold` looped until the cast resolves, stops or runs out.
    pub fn cast(
        &mut self,
        enter: Option<(&'static str, f32)>,
        hold: &'static str,
        footwork: bool,
        seconds: f32,
        now: f64,
    ) {
        self.serial = self.serial.wrapping_add(1);
        let region = if footwork {
            Region::Footwork
        } else {
            Region::Upper
        };
        let (clip, length) = enter.unwrap_or((hold, 0.));
        let mut cast = Strike::new(clip, length, self.serial, now, region);
        cast.then = Some((hold, true));
        cast.until = now + f64::from(seconds.max(0.3));
        self.cast = Some(cast);
    }

    /// The cast shown stopped early (#538): it lets down with `cancel` when the model has it.
    pub fn cancel(&mut self, cancel: Option<(&'static str, f32)>, now: f64) {
        let Some(cast) = self.cast.take().filter(|cast| now < cast.until) else {
            return;
        };
        if let Some((clip, duration)) = cancel {
            self.serial = self.serial.wrapping_add(1);
            self.strike = Some(Strike::new(clip, duration, self.serial, now, cast.region));
        }
    }

    /// Starts a local wind-up of `clip` begun at `started`, or keeps the one showing it; a
    /// different clip supersedes the wind-up shown.
    pub fn prepare(&mut self, clip: &'static str, started: f64) {
        if self
            .preparation
            .is_some_and(|preparation| preparation.clip == clip)
        {
            return;
        }
        self.serial = self.serial.wrapping_add(1);
        self.preparation = Some(Preparation {
            clip,
            serial: self.serial,
            started,
        });
    }

    /// Lets the local wind-up go; it fades out over the upper body's blend.
    pub fn abandon(&mut self) {
        self.preparation = None;
    }

    /// A local wind-up is showing.
    #[must_use]
    pub const fn preparing(&self) -> bool {
        self.preparation.is_some()
    }

    /// The legs' clip while holding a face, ladder or rope (#500): the loop in the direction of
    /// travel along it (up or down first, sideways on a face only), its pace the projected speed
    /// over the clip's `climbSpeedMetersPerSecond`, or the hold when still. A clip the model
    /// lacks keeps the earlier in-place fallback: the walk climbing and the idle holding on.
    fn climb(clips: Clips, available: &impl Fn(&str) -> bool, cues: Cues<'_>) -> Want {
        // `direction` is the travel's yaw from the facing, positive to the left.
        let lateral = -cues.speed * cues.direction.sin();
        let vertical = cues.vertical_speed;
        let climb = clips.climb.filter(|_| cues.traversal.climbing());
        let (idle, up, down, sides) = match (climb, cues.traversal) {
            (Some(c), TraversalView::ClimbingLadder) => {
                (c.ladder_idle, c.ladder_up, c.ladder_down, None)
            }
            (Some(c), TraversalView::ClimbingRope) => (c.rope_idle, c.rope_up, c.rope_down, None),
            (Some(c), _) => (c.idle, c.up, c.down, Some((c.left, c.right))),
            (None, _) => (
                clips.idle,
                clips.walk,
                clips.walk,
                Some((clips.walk, clips.walk)),
            ),
        };
        let ladder = matches!(
            cues.traversal,
            TraversalView::ClimbingLadder | TraversalView::ClimbingRope
        );
        let (clip, along, authored) = if vertical.abs() >= CLIMB_MOVING_SPEED
            && (sides.is_none() || vertical.abs() >= lateral.abs())
        {
            let clip = if vertical > 0. { up } else { down };
            let authored = if ladder {
                LADDER_CLIP_SPEED
            } else {
                CLIMB_CLIP_SPEED
            };
            (clip, vertical.abs(), authored)
        } else if let Some((left, right)) = sides
            && lateral.abs() >= CLIMB_MOVING_SPEED
        {
            let clip = if lateral > 0. { right } else { left };
            (clip, lateral.abs(), CLIMB_SIDE_CLIP_SPEED)
        } else {
            return Want::looped(
                Some(idle).filter(|c| available(c)).unwrap_or(clips.idle),
                1.,
            );
        };
        if !available(clip) {
            return Want::looped(clips.walk, walk_rate(along));
        }
        let authored = cues
            .motions
            .and_then(|motions| motions.get(clip))
            .and_then(|motion| motion.climb_speed)
            .filter(|&speed| speed > 0.)
            .unwrap_or(authored);
        #[allow(clippy::cast_possible_truncation)]
        let rate = (along / authored).clamp(0.5, 1.6) as f32;
        Want::looped(clip, rate)
    }

    /// The mantle a projected mantle of `ledge` begins (#500): a top-out after climbing,
    /// otherwise the ledge's mantle, paced to end about when the zone's does. `None` when the
    /// model lacks it: the earlier in-place fallback (the walk at the mantle's speed) plays.
    fn begin_mantle(
        &self,
        clips: Clips,
        duration: &impl Fn(&str) -> f32,
        cues: Cues<'_>,
        ledge: Ledge,
    ) -> Option<(&'static str, f32)> {
        let climb = clips.climb?;
        let clip = if self.held.climbing() {
            climb.top_out
        } else {
            climb.mantle(ledge)
        };
        let length = f64::from(duration(clip));
        if length <= 0. {
            return None;
        }
        let height = cues
            .motions
            .and_then(|motions| motions.get(clip))
            .and_then(|motion| motion.ledge_height)
            .unwrap_or(match ledge {
                Ledge::Low => 0.7,
                Ledge::Mid => 1.3,
                Ledge::High => 1.9,
            });
        let zone = height / MANTLE_RISE_SPEED + MANTLE_ACROSS_METERS / MANTLE_ACROSS_SPEED;
        #[allow(clippy::cast_possible_truncation)]
        let rate = (length / zone).clamp(1., f64::from(MANTLE_MAX_RATE)) as f32;
        Some((clip, rate))
    }

    /// The legs' gait at `speed` in `direction` from the facing (#538): forward, strafing or
    /// backpedalling at walk or run speed, sprinting beyond [`SPRINT_CLIP_SPEED`]. A gait the
    /// model lacks keeps the earlier choice: the forward run for strafes at run speed, the walk
    /// otherwise.
    fn gait(
        &self,
        clips: Clips,
        available: &impl Fn(&str) -> bool,
        speed: f64,
        direction: f64,
    ) -> Want {
        let gait = |pick: fn(GaitClips) -> &'static str| {
            clips.gaits.map(pick).filter(|clip| available(clip))
        };
        let angle = direction.abs();
        let left = direction > 0.;
        #[allow(clippy::cast_possible_truncation)]
        let run_rate = (speed / RUN_CLIP_GROUND).clamp(0.7, 1.5) as f32;
        if speed > RUN_CLIP_SPEED {
            let directed = if angle <= STRAFE_ANGLE {
                if speed > SPRINT_CLIP_SPEED
                    && let Some(sprint) = gait(|gaits| gaits.sprint)
                {
                    #[allow(clippy::cast_possible_truncation)]
                    let rate = (speed / SPRINT_CLIP_GROUND).clamp(0.7, 1.5) as f32;
                    return Want::looped(sprint, rate);
                }
                Some(clips.run)
            } else if angle >= BACKPEDAL_ANGLE {
                gait(|gaits| gaits.run_back)
            } else if left {
                gait(|gaits| gaits.run_left)
            } else {
                gait(|gaits| gaits.run_right)
            };
            match directed {
                Some(clip) => Want::looped(clip, run_rate),
                // Running forward or strafing at the player's run speed plays the run (#515);
                // backpedalling keeps the walk.
                None if angle <= FRAC_PI_2 => Want::looped(clips.run, run_rate),
                None => Want::looped(clips.walk, walk_rate(speed)),
            }
        } else {
            let directed = if angle <= STRAFE_ANGLE {
                None
            } else if angle >= BACKPEDAL_ANGLE {
                gait(|gaits| gaits.walk_back)
            } else if left {
                gait(|gaits| gaits.walk_left)
            } else {
                gait(|gaits| gaits.walk_right)
            };
            Want::looped(directed.unwrap_or(clips.walk), walk_rate(speed))
        }
    }

    /// A standing unit turning past [`TURN_CLIP_RATE`] steps round with its turn clip (#538),
    /// at the pace of the turn; a guard holds its stance instead.
    fn turn(
        &mut self,
        clips: Clips,
        duration: &impl Fn(&str) -> f32,
        cues: Cues<'_>,
        guarding: bool,
    ) -> Option<Want> {
        let gaits = clips
            .gaits
            .filter(|_| !guarding && !cues.dead && !cues.reduced)?;
        if cues.turn.abs() > TURN_CLIP_RATE {
            self.turning = Some((cues.turn > 0., cues.now + TURN_HOLD_SECONDS));
        }
        let (left, _) = self.turning.filter(|&(_, until)| cues.now < until)?;
        let clip = if left {
            gaits.turn_left
        } else {
            gaits.turn_right
        };
        let length = f64::from(duration(clip));
        if length <= 0. {
            return None;
        }
        // Through the hold the step keeps at least the threshold's pace.
        #[allow(clippy::cast_possible_truncation)]
        let rate = (cues.turn.abs().max(TURN_CLIP_RATE) * length / TURN_CLIP_RADIANS)
            .clamp(0.7, 1.6) as f32;
        Some(Want::looped(clip, rate))
    }

    /// Advances the clocks and returns the layers to pose this frame; `duration` gives a clip's
    /// length in seconds (zero when the model lacks it).
    pub fn advance(
        &mut self,
        clips: Clips,
        duration: impl Fn(&str) -> f32,
        cues: Cues<'_>,
        blend: Blend,
    ) -> Animation {
        let Cues {
            speed,
            vertical_speed,
            dead,
            guarding,
            now,
            reduced,
            ..
        } = cues;
        let blend = blend.clamped();
        #[allow(clippy::cast_possible_truncation)]
        let dt = cues.dt as f32;
        let available = |clip: &str| duration(clip) > 0.;
        let landed = clips.jump.is_none_or(|jump| {
            self.lower
                .playing(jump.land)
                .is_none_or(|play| play.time >= duration(jump.land))
        });
        // The zone's water mode, never the rendered water, decides the swim clips (Atlas 6.6.41).
        let swims = cues.water.swims() && !dead;
        let swim = |pick: fn(SwimClips) -> &'static str| {
            clips.swim.map(pick).filter(|clip| available(clip))
        };
        if let Some(clip) = self.crossing
            && self
                .lower
                .playing(clip)
                .is_none_or(|play| play.time >= duration(clip))
        {
            self.crossing = None;
        }
        if swims != self.swimming {
            self.crossings = self.crossings.wrapping_add(1);
            self.crossed = Some(now);
            self.crossing = if reduced || self.base.current.is_none() {
                None
            } else if swims {
                swim(|clips| clips.enter)
            } else {
                swim(|clips| clips.exit)
            };
        }
        self.swimming = swims;
        // The zone's climb or mantle (#500) picks the traversal clips. A mantle begun from a
        // climb tops out; letting go of a face plays the let-go over the fall.
        let held = if dead {
            TraversalView::Free
        } else {
            cues.traversal
        };
        if let Some(ledge) = held.mantling()
            && self.held.mantling().is_none()
        {
            self.pulls = self.pulls.wrapping_add(1);
            self.mantle = self.begin_mantle(clips, &duration, cues, ledge);
        }
        if self.held.climbing() && !held.holds() {
            self.pulls = self.pulls.wrapping_add(1);
            self.letting_go = clips
                .climb
                .map(|climb| (climb.let_go, now))
                .filter(|&(clip, _)| !reduced && available(clip));
        }
        self.held = held;
        // A mantle plays out after the zone's ends, until the unit moves on.
        if let Some((clip, _)) = self.mantle
            && held.mantling().is_none()
            && (dead
                || speed > 0.25
                || self
                    .base
                    .playing(clip)
                    .is_none_or(|play| play.time >= duration(clip)))
        {
            self.mantle = None;
        }
        if let Some((clip, started)) = self.letting_go
            && (dead || held.holds() || now - started >= f64::from(duration(clip)))
        {
            self.letting_go = None;
        }
        let traversing = held.holds() || self.mantle.is_some();
        // The projection already supplies cosmetic vertical velocity. Retain the flight phase
        // through the apex instead of treating its zero velocity as a landing. This is a
        // visual cue, not a grounded/physics decision. The water carries a swimmer: it neither
        // jumps, falls nor lands; nor does a climber or a mantler, whom the zone carries.
        self.flight = if dead || swims || traversing {
            Flight::Ground
        } else if vertical_speed > 0.3 {
            match self.flight {
                Flight::Rising | Flight::Airborne => self.flight,
                _ => Flight::Rising,
            }
        } else if vertical_speed < -0.3 {
            Flight::Falling
        } else {
            match self.flight {
                Flight::Falling if reduced => Flight::Ground,
                Flight::Falling => {
                    self.landing = self.landing.wrapping_add(1);
                    Flight::Landing
                }
                Flight::Landing if landed => Flight::Ground,
                other => other,
            }
        };
        if let Some(jump) = clips.jump
            && matches!(self.flight, Flight::Rising)
            && self
                .base
                .playing(jump.start)
                .is_some_and(|play| play.time >= duration(jump.start))
        {
            self.flight = Flight::Airborne;
        }
        // A guard the zone starts protecting ends the strike shown before it; a strike accepted
        // while it protects still plays over it.
        if dead || guarding && !self.guarding {
            self.strike = None;
            self.cast = None;
            self.preparation = None;
        }
        self.guarding = guarding;
        self.strike = self.strike.filter(|strike| now < strike.until);
        self.cast = self.cast.filter(|cast| now < cast.until);
        // A swimmer, a climber or a mantler has no footing for a strike's footwork.
        let standing =
            matches!(self.flight, Flight::Ground) && speed <= 0.25 && !swims && !traversing;
        let layered = clips.upper_body.is_some();
        let mut splits = [false; 2];
        for (strike, split) in [&mut self.strike, &mut self.cast]
            .into_iter()
            .zip(&mut splits)
        {
            if let Some(strike) = strike {
                let full_body =
                    strike.full_body && (standing || !layered || strike.region == Region::Whole);
                *split = strike.full_body && !full_body;
                strike.full_body = full_body;
            }
        }
        // Only the one shown leaves the legs.
        let split = if self.strike.is_some() {
            splits[0]
        } else {
            splits[1]
        };
        // A strike or reaction plays over the cast it interrupts for a moment.
        let strike = self
            .strike
            .or(self.cast)
            .filter(|strike| !reduced && (strike.full_body || layered))
            .map(|strike| (strike.full_body, strike.want(now)));
        // An accepted strike outranks the local wind-up; the wind-up never moves the legs.
        let prepared = self
            .preparation
            .filter(|preparation| {
                strike.is_none() && !reduced && layered && available(preparation.clip)
            })
            .map(|preparation| Want {
                // The pinned time is the wind-up's clock; a still rate keeps an abandoned wind-up
                // from playing on toward the impact while it fades out.
                rate: 0.,
                at: Some(preparation.at(now, duration(preparation.clip))),
                ..Want::hold(preparation.clip, preparation.serial)
            });
        let ground = if let Some((clip, rate)) = self.mantle {
            Want {
                rate,
                ..Want::hold(clip, self.pulls)
            }
        } else if held.climbing() {
            self.turning = None;
            Self::climb(clips, &available, cues)
        } else if held.mantling().is_some() {
            // No mantle clip: the earlier in-place fallback, the walk at the mantle's pace.
            let travel = speed.hypot(vertical_speed);
            if travel > 0.25 {
                Want::looped(clips.walk, walk_rate(travel))
            } else {
                Want::looped(clips.idle, 1.)
            }
        } else if swims {
            // Strokes keep pace with the projected travel; a missing clip falls back to the
            // leaned walk for travel and the idle for treading, in place either way.
            let travel = speed.hypot(vertical_speed);
            #[allow(clippy::cast_possible_truncation)]
            let stroke_rate = (travel / STROKE_SPEED).clamp(0.6, 1.5) as f32;
            let vertical = if vertical_speed < -DIVE_SPEED {
                swim(|clips| clips.descend)
            } else if vertical_speed > DIVE_SPEED {
                swim(|clips| clips.ascend)
            } else {
                None
            };
            if let Some(clip) = vertical {
                Want::looped(clip, stroke_rate)
            } else if travel > 0.25 {
                match swim(|clips| clips.stroke) {
                    Some(stroke) => Want::looped(stroke, stroke_rate),
                    None => Want::looped(clips.walk, walk_rate(travel)),
                }
            } else {
                Want::looped(swim(|clips| clips.tread).unwrap_or(clips.idle), 1.)
            }
        } else if let Some(wade) = swim(|clips| clips.wade)
            && cues.water == WaterMode::Wading
            && speed > 0.25
        {
            Want::looped(wade, walk_rate(speed))
        } else if speed > 0.25 {
            self.turning = None;
            self.gait(clips, &available, speed, cues.direction)
        } else if let Some(turn) = self.turn(clips, &duration, cues, guarding) {
            turn
        } else {
            // A standing protecting guard holds the shield up with its own stance.
            Want::looped(clips.guard.filter(|_| guarding).unwrap_or(clips.idle), 1.)
        };
        // A dodge roll (#516) plays its clip once over the roll's length, whatever the legs did.
        if cues.rolling && !self.rolling {
            self.rolls = self.rolls.wrapping_add(1);
        }
        self.rolling = cues.rolling;
        let roll = clips
            .roll
            .filter(|&clip| cues.rolling && !dead && !swims && !reduced && available(clip));
        let base = if dead {
            Want::hold(clips.death, 0)
        } else if let Some(clip) = roll {
            #[allow(clippy::cast_possible_truncation)]
            let rate = (f64::from(duration(clip)) / ROLL_SECONDS) as f32;
            Want {
                rate,
                ..Want::hold(clip, self.rolls)
            }
        } else {
            match (clips.jump, self.flight) {
                (Some(jump), Flight::Rising) if !reduced && available(jump.start) => {
                    Want::hold(jump.start, self.landing)
                }
                (Some(jump), Flight::Rising | Flight::Airborne | Flight::Falling)
                    if available(jump.airborne) =>
                {
                    Want::looped(jump.airborne, 1.)
                }
                _ => ground,
            }
        };
        let lower = if dead || roll.is_some() {
            None
        } else if let Some((true, want)) = strike {
            Some(want)
        } else if let (Some(jump), Flight::Landing) = (clips.jump, self.flight)
            && !reduced
            && available(jump.land)
        {
            Some(Want::hold(jump.land, self.landing))
        } else if let Some((clip, _)) = self.letting_go {
            Some(Want::hold(clip, self.pulls))
        } else {
            self.crossing.map(|clip| Want::hold(clip, self.crossings))
        };
        let upper = if dead || !layered {
            None
        } else if let Some((false, want)) = strike {
            Some(want)
        } else if prepared.is_some() {
            prepared
        } else if let Some(guard) = clips.guard
            && guarding
            && base.clip != guard
            && available(guard)
        {
            Some(Want::looped(guard, 1.))
        } else {
            None
        };
        // The first clip a unit shows has nothing to blend from. Going into or out of swimming
        // depth blends a little longer, so the body eases between land and water.
        let first = self.base.current.is_none();
        let lower_seconds = if self
            .crossed
            .is_some_and(|crossed| now - crossed < f64::from(WATER_BLEND_SECONDS))
        {
            blend.lower_seconds.max(WATER_BLEND_SECONDS)
        } else {
            blend.lower_seconds
        };
        let seconds = if first { 0. } else { lower_seconds };
        self.base.advance(Some(base), dt, seconds, reduced);
        self.lower.advance(lower, dt, lower_seconds, reduced);
        if roll.is_some() {
            // The forward roll turns the drawn body at once toward the roll's travel (#516), and
            // eases back to the facing after it; a roll stopped by a wall keeps its heading.
            let heading = if speed > 1. {
                (cues.direction + std::f64::consts::PI).rem_euclid(std::f64::consts::TAU)
                    - std::f64::consts::PI
            } else {
                self.attitude.heading
            };
            self.attitude = Attitude { pitch: 0., heading };
        } else {
            let attitude = Attitude::target(
                if dead { WaterMode::Dry } else { cues.water },
                speed,
                cues.direction,
                vertical_speed,
            );
            self.attitude = self.attitude.settle(attitude, cues.dt, reduced || first);
        }
        if split {
            // The strike leaves the legs mid-swing: the upper body keeps it without a blend.
            self.upper = Track::default();
            self.upper.advance(upper, dt, 0., reduced);
        } else {
            self.upper.advance(upper, dt, blend.upper_seconds, reduced);
        }
        let mut over = Vec::new();
        self.lower
            .layers(&duration, blend.lower_weight, Mask::Lower, &mut over);
        let over_weight: f32 = over.iter().map(|layer| layer.weight).sum();
        let mut legs = Vec::new();
        self.base
            .layers(&duration, 1. - over_weight, Mask::Lower, &mut legs);
        legs.extend(over);
        let mut arms = Vec::new();
        self.upper
            .layers(&duration, blend.upper_weight, Mask::Upper, &mut arms);
        let arms_weight: f32 = arms.iter().map(|layer| layer.weight).sum();
        let layers = if arms_weight <= 1e-3 {
            // The upper body follows the legs: one whole-body set of layers.
            legs.into_iter()
                .map(|layer| Layer {
                    mask: Mask::All,
                    ..layer
                })
                .collect()
        } else {
            let follow: Vec<_> = legs
                .iter()
                .filter(|layer| layer.weight * (1. - arms_weight) > 1e-3)
                .map(|layer| Layer {
                    weight: layer.weight * (1. - arms_weight),
                    mask: Mask::Upper,
                    ..*layer
                })
                .collect();
            legs.into_iter().chain(follow).chain(arms).collect()
        };
        let lower = self
            .lower
            .target(&duration)
            .or_else(|| self.base.target(&duration))
            .unwrap_or((clips.idle, 0.));
        let upper = self.upper.target(&duration).unwrap_or(lower);
        Animation {
            layers,
            lower,
            upper,
            attitude: self.attitude,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mmorpg_core::{CreatureId, NpcId, greyhaven_vale};
    use strum::VariantArray;

    #[test]
    fn named_greyhaven_residents_have_presets_and_classes_their_weapons() {
        let content = greyhaven_vale::content();
        let presets: Vec<_> = content
            .npcs()
            .iter()
            .map(|npc| Resident::of_name(&npc.name).map(Resident::preset))
            .collect();
        assert_eq!(
            presets,
            [
                Some("resident.marshal"),
                Some("resident.tanner"),
                Some("resident.innkeeper"),
                Some("resident.farmer"),
                Some("resident.healer"),
                Some("resident.guard"),
                Some("resident.guard"),
                Some("resident.guard"),
                Some("resident.guard"),
            ]
        );
        let names: Vec<_> = PlayerClass::VARIANTS
            .iter()
            .copied()
            .map(|class| Loadout::of_class(class).name())
            .collect();
        assert_eq!(
            names,
            [
                "sword-and-shield",
                "bow-and-quiver",
                "staff.crystal-and-tome",
                "staff.bone",
                "dagger",
                "mace.flanged",
                "staff.gnarled",
            ]
        );
    }

    #[test]
    fn humanoid_enemies_have_presets() {
        let content = greyhaven_vale::content();
        let presets: Vec<_> = content
            .creature_templates()
            .iter()
            .filter_map(|template| Resident::of_creature(&template.name).map(Resident::preset))
            .collect();
        assert_eq!(
            presets,
            [
                "enemy.marauder",
                "enemy.redbrand.bandit",
                "enemy.redbrand.garrick",
                "enemy.redbrand.crossbowman",
                "enemy.redbrand.brute",
            ]
        );
        // Each is authored in the shared presets both clients draw.
        assert!(
            presets
                .iter()
                .all(|id| super::super::presets::ENEMY_PRESET_IDS.contains(id))
        );
    }

    #[test]
    fn classes_wear_their_starter_outfit_and_drops_replace_its_slot_pieces() {
        let presets: Vec<_> = PlayerClass::VARIANTS
            .iter()
            .copied()
            .map(|class| Outfit::new(class, None).preset())
            .collect();
        assert_eq!(
            presets,
            [
                "class.warden",
                "class.ranger",
                "class.arcanist",
                "class.necromancer",
                "class.rogue",
                "class.cleric",
                "class.druid",
            ]
        );
        assert!(Outfit::new(PlayerClass::Warden, None).replaced().is_empty());
        // The Cloth Hood (6) and Worn Boots (9) cover the head and feet sockets only.
        let mut slots = [None; mmorpg_core::EQUIPMENT_SLOTS];
        slots[EquipmentSlot::Head.index() as usize] = Some(mmorpg_core::ItemId::new(6));
        slots[EquipmentSlot::Feet.index() as usize] = Some(mmorpg_core::ItemId::new(9));
        let equipment = Equipment::from_slots(slots).unwrap();
        assert_eq!(
            Outfit::new(PlayerClass::Warden, Some(&equipment)).replaced(),
            ["head.armor", "foot_l.armor", "foot_r.armor"]
        );
        // The Cleric's gold accent (#d9b44a), linear.
        let [r, g, b] = Outfit::new(PlayerClass::Cleric, None).trim();
        assert!(r > g && g > b && (r - 0.693).abs() < 0.01);
    }

    #[test]
    fn players_are_the_ranger_and_only_animal_families_import() {
        let content = greyhaven_vale::content();
        let creature = |family: CreatureFamily| {
            let spawn = content
                .creature_spawns()
                .iter()
                .find(|spawn| content.creature_template(spawn.template).unwrap().family == family)
                .unwrap();
            let mut record = content_record();
            record.appearance = spawn.template.get();
            Kind::of(EntityRef::Creature(CreatureId::new(1)), &record, &content)
        };
        assert_eq!(
            Kind::of(EntityRef::Player(1), &content_record(), &content),
            Some(Kind::Ranger)
        );
        assert_eq!(creature(CreatureFamily::Wolf), Some(Kind::Wolf));
        assert_eq!(creature(CreatureFamily::Vermin), Some(Kind::Rat));
        assert_eq!(creature(CreatureFamily::Boar), Some(Kind::Pig));
        assert_eq!(creature(CreatureFamily::Redbrand), None);
        assert_eq!(
            Kind::of(EntityRef::Npc(NpcId::new(1)), &content_record(), &content),
            None
        );
    }

    fn content_record() -> EntitySnapshot {
        EntitySnapshot {
            kind: mmorpg_core::EntityKind::Player,
            id: 1,
            appearance: 0,
            position: [0; 3],
            velocity: [0; 3],
            facing: 0,
            level: 1,
            health_percent: 100,
            flags: mmorpg_core::EntityFlags::default(),
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn advance(
        playback: &mut Playback,
        clips: Clips,
        duration: impl Fn(&str) -> f32,
        speed: f64,
        vertical_speed: f64,
        dead: bool,
        now: f64,
        dt: f64,
        reduced: bool,
        guarding: bool,
    ) -> Animation {
        playback.advance(
            clips,
            duration,
            Cues {
                speed,
                vertical_speed,
                dead,
                guarding,
                now,
                dt,
                reduced,
                ..Cues::default()
            },
            Blend::default(),
        )
    }

    /// Each region's weights: (upper, lower), counting a whole-body layer in both.
    fn region_weights(animation: &Animation) -> (f32, f32) {
        let sum = |upper: bool| {
            animation
                .layers
                .iter()
                .filter(|layer| layer.mask.drives(upper))
                .map(|layer| layer.weight)
                .sum()
        };
        (sum(true), sum(false))
    }

    #[test]
    fn directional_gaits_follow_the_travel_and_fall_back_to_the_walk_and_run() {
        let gait = |speed: f64, direction: f64, gaits: bool| {
            Playback::default()
                .advance(
                    Kind::Ranger.clips(),
                    |clip| {
                        if gaits || !(clip.contains("Bwd") || clip.contains("Strafe")) {
                            1.
                        } else {
                            0.
                        }
                    },
                    Cues {
                        speed,
                        direction,
                        dt: 0.1,
                        ..Cues::default()
                    },
                    Blend::default(),
                )
                .lower
                .0
        };
        let run = f64::from(mmorpg_core::RUN_SPEED_UNITS_PER_SECOND) / 100.;
        let backpedal = f64::from(mmorpg_core::BACKPEDAL_SPEED_UNITS_PER_SECOND) / 100.;
        let pi = std::f64::consts::PI;
        assert_eq!(gait(run, 0., true), "Jog_Fwd_Loop");
        // A diagonal stays forward.
        assert_eq!(gait(run, pi / 4., true), "Jog_Fwd_Loop");
        assert_eq!(gait(run, FRAC_PI_2, true), "Jog_Strafe_Left_Loop");
        assert_eq!(gait(run, -FRAC_PI_2, true), "Jog_Strafe_Right_Loop");
        assert_eq!(gait(run, pi, true), "Jog_Bwd_Loop");
        assert_eq!(gait(backpedal, -2.4, true), "Jog_Bwd_Loop");
        assert_eq!(gait(run / 2., pi, true), "Walk_Bwd_Loop");
        assert_eq!(gait(run / 2., FRAC_PI_2, true), "Walk_Strafe_Left_Loop");
        assert_eq!(gait(run / 2., 0., true), "Walk_Loop");
        assert_eq!(gait(SPRINT_CLIP_SPEED + 1., 0., true), "Sprint_Loop");
        // Without the directional clips: the run for strafes, the walk for backpedalling.
        assert_eq!(gait(run, FRAC_PI_2, false), "Jog_Fwd_Loop");
        assert_eq!(gait(run, pi, false), "Walk_Loop");
        assert_eq!(gait(backpedal, pi, false), "Walk_Loop");
        assert_eq!(gait(run / 2., FRAC_PI_2, false), "Walk_Loop");
    }

    #[test]
    fn a_standing_turn_steps_round_and_settles_back_to_idle() {
        let mut playback = Playback::default();
        let mut frame = |turn: f64, now: f64| {
            playback
                .advance(
                    Kind::Ranger.clips(),
                    |_| 1.,
                    Cues {
                        turn,
                        now,
                        dt: 0.05,
                        ..Cues::default()
                    },
                    Blend::default(),
                )
                .lower
                .0
        };
        assert_eq!(frame(0.5, 0.), "Idle_Loop");
        assert_eq!(frame(3., 0.05), "Turn_Left");
        assert_eq!(frame(0., 0.1), "Turn_Left", "the step holds briefly");
        assert_eq!(frame(-3., 0.15), "Turn_Right");
        assert_eq!(frame(0., 0.6), "Idle_Loop");
    }

    #[test]
    fn the_bow_cycle_holds_the_bow_and_its_draw_samples_between_frames() {
        let shows = |clip: &'static str| Animation {
            layers: vec![Layer::single(clip, 0.2)],
            lower: (clip, 0.2),
            upper: (clip, 0.2),
            attitude: Attitude::default(),
        };
        for clip in [
            "Bow_Aim_Loop",
            "Bow_Draw",
            "Bow_Hold_Loop",
            "Bow_Release",
            "Bow_Recovery",
            "Bow_Cancel",
        ] {
            assert_eq!(
                Kind::Ranger.bow_shot(&shows(clip)),
                Some(BowShot { clip, time: 0.2 })
            );
        }
        for clip in ["Idle_Loop", "Spell_Simple_Enter", "Sword_Attack"] {
            assert_eq!(Kind::Ranger.bow_shot(&shows(clip)), None, "{clip}");
        }
        assert_eq!(Kind::Wolf.bow_shot(&shows("Bow_Draw")), None);
        let table = [0.2, 0.4, 0.5];
        assert!((draw_meters(&table, 10., 0.05) - 0.3).abs() < 1e-6);
        assert!((draw_meters(&table, 10., 9.) - 0.5).abs() < 1e-6);
        assert!((draw_meters(&table, 10., -1.) - 0.2).abs() < 1e-6);
        assert!(draw_meters(&[], 30., 0.5).abs() < f32::EPSILON);
    }

    #[test]
    fn reactions_casts_and_the_bow_cycle_play_their_clips_and_fall_back() {
        let clips = Kind::Ranger.clips();
        let frame = |playback: &mut Playback, speed: f64, now: f64| {
            playback.advance(
                clips,
                |_| 1.,
                Cues {
                    speed,
                    now,
                    dt: 0.05,
                    ..Cues::default()
                },
                Blend::default(),
            )
        };
        // A hit flinches the whole body standing and the upper body on the move.
        let mut playback = Playback::default();
        playback.react(Reaction::Chest, "Hit_Chest", 0.33, 0.);
        assert_eq!(frame(&mut playback, 0., 0.05).lower.0, "Hit_Chest");
        let moving = frame(&mut playback, 4., 0.1);
        assert_eq!(moving.upper.0, "Hit_Chest");
        assert_ne!(moving.lower.0, "Hit_Chest");
        // A knockback throws the whole body even while the knockback moves it.
        playback.react(Reaction::Knockback, "Hit_Knockback", 0.83, 1.);
        assert_eq!(frame(&mut playback, 6., 1.05).lower.0, "Hit_Knockback");
        // A block is the shield arm's; an ordinary hit never cuts a strike short.
        let mut playback = Playback::default();
        playback.react(Reaction::Block, "Sword_Block", 1.2, 0.);
        let blocked = frame(&mut playback, 0., 0.05);
        assert_eq!(blocked.upper.0, "Sword_Block");
        assert_eq!(blocked.lower.0, "Idle_Loop");
        playback.strike("Sword_Attack", 1.5, 0.1);
        playback.react(Reaction::Chest, "Hit_Chest", 0.33, 0.2);
        assert_eq!(frame(&mut playback, 0., 0.25).upper.0, "Sword_Attack");
        // A spell cast raises, then holds on the upper body until it resolves.
        let mut playback = Playback::default();
        playback.cast(
            Some(("Spell_Simple_Enter", 0.5)),
            "Spell_Simple_Idle_Loop",
            false,
            2.,
            0.,
        );
        let entering = frame(&mut playback, 0., 0.1);
        assert_eq!(entering.upper.0, "Spell_Simple_Enter");
        assert_eq!(entering.lower.0, "Idle_Loop");
        assert_eq!(
            frame(&mut playback, 0., 0.8).upper.0,
            "Spell_Simple_Idle_Loop"
        );
        playback.cancel(Some(("Spell_Simple_Exit", 0.4)), 1.);
        assert_eq!(frame(&mut playback, 0., 1.1).upper.0, "Spell_Simple_Exit");
        // The bow draws and holds through a cast, then releases and recovers, whole body.
        let mut playback = Playback::default();
        playback.cast(Some(("Bow_Draw", 0.7)), "Bow_Hold_Loop", true, 2., 0.);
        assert_eq!(frame(&mut playback, 0., 0.1).lower.0, "Bow_Draw");
        assert_eq!(frame(&mut playback, 0., 1.).lower.0, "Bow_Hold_Loop");
        playback.strike("Bow_Release", 0.47, 2.);
        playback.recover("Bow_Recovery", 1.1);
        assert_eq!(frame(&mut playback, 0., 2.1).lower.0, "Bow_Release");
        assert_eq!(frame(&mut playback, 0., 2.8).lower.0, "Bow_Recovery");
        assert_eq!(frame(&mut playback, 0., 3.8).lower.0, "Idle_Loop");
        // A model without casts or reactions has none to start.
        assert!(Kind::Wolf.cast(Some(PlayerClass::Ranger)).is_none());
        assert_eq!(Kind::Wolf.reaction(Reaction::Chest), None);
        assert_eq!(
            Kind::Ranger.cycle_of("Bow_Release").and_then(|c| c.aim),
            Some("Bow_Aim_Loop")
        );
    }

    #[test]
    fn clip_choice_follows_guard_speed_attack_and_death() {
        let clips = Kind::Ranger.clips();
        let one = |_: &str| 1.;
        let mut playback = Playback::default();
        assert_eq!(
            advance(
                &mut playback,
                clips,
                one,
                0.,
                0.,
                false,
                0.,
                0.1,
                false,
                true
            )
            .upper
            .0,
            "Idle_Shield_Loop"
        );
        // Walking keeps the walk on the legs while the protecting guard holds the shield up.
        let walking = advance(
            &mut playback,
            clips,
            one,
            1.,
            0.,
            false,
            0.,
            0.1,
            false,
            true,
        );
        assert_eq!(walking.lower.0, "Walk_Loop");
        assert_eq!(walking.upper.0, "Idle_Shield_Loop");
        assert_eq!(
            advance(
                &mut Playback::default(),
                Kind::Wolf.clips(),
                one,
                0.,
                0.,
                false,
                0.,
                0.1,
                false,
                true
            )
            .upper
            .0,
            "Idle"
        );
        let mut playback = Playback::default();
        assert_eq!(
            advance(
                &mut playback,
                clips,
                one,
                0.,
                0.,
                false,
                0.,
                0.1,
                false,
                false
            )
            .upper
            .0,
            "Idle_Loop"
        );
        assert_eq!(
            advance(
                &mut playback,
                clips,
                one,
                4.,
                0.,
                false,
                0.,
                0.1,
                false,
                false
            )
            .upper
            .0,
            "Jog_Fwd_Loop"
        );
        playback.strike("Sword_Attack", 1., 0.);
        let running = advance(
            &mut playback,
            clips,
            one,
            4.,
            0.,
            false,
            0.1,
            0.1,
            false,
            false,
        );
        assert_eq!(running.upper.0, "Sword_Attack");
        assert_eq!(running.lower.0, "Jog_Fwd_Loop");
        advance(
            &mut playback,
            clips,
            one,
            0.,
            0.,
            true,
            1.,
            0.1,
            false,
            false,
        );
        assert_eq!(
            advance(
                &mut playback,
                clips,
                one,
                0.,
                0.,
                true,
                5.,
                5.,
                false,
                false
            )
            .upper,
            ("Death01", 1.)
        );
        let mut frozen = Playback::default();
        advance(&mut frozen, clips, one, 0., 0., false, 0., 0.1, true, false);
        assert_eq!(
            advance(&mut frozen, clips, one, 0., 0., false, 1., 0.5, true, false)
                .upper
                .1,
            0.
        );
    }

    #[test]
    fn authored_jump_tracks_ascent_apex_descent_and_landing() {
        let mut playback = Playback::default();
        let clips = Kind::Ranger.clips();
        let duration = |_: &str| 0.2;
        let mut pose = |vertical, dt| {
            advance(
                &mut playback,
                clips,
                duration,
                0.,
                vertical,
                false,
                0.,
                dt,
                false,
                false,
            )
            .upper
            .0
        };
        assert_eq!(pose(4.8, 0.1), "Jump_Start");
        pose(3., 0.2);
        assert_eq!(pose(1., 0.1), "Jump_Loop");
        assert_eq!(pose(0., 0.1), "Jump_Loop");
        assert_eq!(pose(-2., 0.1), "Jump_Loop");
        assert_eq!(pose(0., 0.1), "Jump_Land");
        pose(0., 0.2);
        assert_eq!(pose(0., 0.1), "Idle_Loop");
    }

    #[test]
    fn jump_respects_missing_clips_reduced_motion_and_death() {
        let mut playback = Playback::default();
        let clips = Kind::Ranger.clips();
        let missing = |name: &str| if name.starts_with("Jump_") { 0. } else { 1. };
        assert_eq!(
            advance(
                &mut playback,
                clips,
                missing,
                4.,
                4.8,
                false,
                0.,
                0.1,
                false,
                false
            )
            .upper
            .0,
            "Jog_Fwd_Loop"
        );
        let one = |_: &str| 1.;
        assert_eq!(
            advance(
                &mut playback,
                clips,
                one,
                0.,
                -2.,
                false,
                0.,
                0.1,
                true,
                false
            )
            .upper,
            ("Jump_Loop", 0.)
        );
        assert_eq!(
            advance(
                &mut playback,
                clips,
                one,
                0.,
                -2.,
                true,
                0.,
                0.1,
                false,
                false
            )
            .upper
            .0,
            "Death01"
        );
    }

    #[test]
    fn a_local_wind_up_holds_before_impact_then_commits_or_fades() {
        let clips = Kind::Ranger.clips();
        let duration = |clip: &str| if clip == "Sword_Attack" { 1.5 } else { 1. };
        let frame = |playback: &mut Playback, now: &mut f64| {
            *now += 0.05;
            advance(
                playback, clips, duration, 0., 0., false, *now, 0.05, false, false,
            )
        };
        let mut playback = Playback::default();
        let mut now = 0.;
        frame(&mut playback, &mut now);
        playback.prepare("Sword_Attack", now);
        for _ in 0..20 {
            frame(&mut playback, &mut now);
        }
        // An unanswered wind-up holds short of the swing, on the arms only.
        let held = frame(&mut playback, &mut now);
        assert_eq!(held.upper, ("Sword_Attack", WINDUP_HOLD_SECONDS));
        assert_eq!(held.lower.0, "Idle_Loop");
        // The accepted strike continues from the hold instead of restarting.
        playback.commit("Sword_Attack", 1.5, now);
        assert!(!playback.preparing());
        let swing = frame(&mut playback, &mut now);
        assert_eq!(swing.lower.0, "Sword_Attack");
        assert!(
            (swing.lower.1 - (WINDUP_HOLD_SECONDS + 0.05)).abs() < 1e-3,
            "{swing:?}"
        );

        // A refusal lets it fade out without any swing.
        let mut refused = Playback::default();
        let mut now = 0.;
        frame(&mut refused, &mut now);
        refused.prepare("Sword_Attack", now);
        for _ in 0..10 {
            frame(&mut refused, &mut now);
        }
        refused.abandon();
        let fading = frame(&mut refused, &mut now);
        assert!(
            fading
                .layers
                .iter()
                .any(|layer| layer.clip == "Sword_Attack")
        );
        assert_eq!(fading.lower.0, "Idle_Loop");
        // The fading wind-up stays held: it never plays on toward the impact.
        for _ in 0..10 {
            let frame = frame(&mut refused, &mut now);
            assert!(
                frame
                    .layers
                    .iter()
                    .all(|layer| layer.clip != "Sword_Attack" || layer.time <= WINDUP_HOLD_SECONDS),
                "{frame:?}"
            );
        }
        assert!(
            frame(&mut refused, &mut now)
                .layers
                .iter()
                .all(|layer| layer.clip == "Idle_Loop")
        );
    }

    #[test]
    fn an_aerial_swing_continues_on_the_upper_body_through_touchdown() {
        let clips = Kind::Ranger.clips();
        let duration = |clip: &str| if clip == "Sword_Attack" { 1.5 } else { 0.3 };
        let mut playback = Playback::default();
        let mut now = 0.;
        let frame = |playback: &mut Playback, now: &mut f64, vertical: f64, guarding: bool| {
            *now += 0.05;
            advance(
                playback, clips, duration, 0., vertical, false, *now, 0.05, false, guarding,
            )
        };
        frame(&mut playback, &mut now, 4.8, false);
        playback.strike("Sword_Attack", 1.5, now);
        let swing = frame(&mut playback, &mut now, 2., false);
        assert_eq!(swing.upper.0, "Sword_Attack");
        assert_eq!(swing.lower.0, "Jump_Start");
        frame(&mut playback, &mut now, -2., false);
        // Touchdown: the legs land while the swing keeps its own clock on the upper body.
        let touchdown = frame(&mut playback, &mut now, 0., false);
        assert_eq!(touchdown.lower.0, "Jump_Land");
        assert_eq!(touchdown.upper, ("Sword_Attack", 0.15));
        let (upper, lower) = region_weights(&touchdown);
        assert!(
            (upper - 1.).abs() < 1e-4 && (lower - 1.).abs() < 1e-4,
            "{touchdown:?}"
        );
        assert!(
            touchdown
                .layers
                .iter()
                .all(|layer| layer.clip != "Sword_Attack" || layer.mask == Mask::Upper)
        );
        // After the landing the swing stays off the legs: no whole-body strike re-enters mid-swing.
        for _ in 0..8 {
            frame(&mut playback, &mut now, 0., false);
        }
        let standing = frame(&mut playback, &mut now, 0., false);
        assert_eq!(standing.lower.0, "Idle_Loop");
        assert_eq!(standing.upper.0, "Sword_Attack");
        // The zone's guard starting to protect ends the shown swing and blends the shield in.
        let guard = frame(&mut playback, &mut now, 0., true);
        assert_eq!(guard.lower.0, "Idle_Shield_Loop");
        assert_eq!(guard.upper.0, "Idle_Shield_Loop");
        assert!(
            guard
                .layers
                .iter()
                .any(|layer| layer.clip == "Sword_Attack"),
            "the swing fades out"
        );
        for _ in 0..4 {
            frame(&mut playback, &mut now, 0., true);
        }
        assert!(
            frame(&mut playback, &mut now, 0., true)
                .layers
                .iter()
                .all(|layer| layer.clip == "Idle_Shield_Loop" && layer.mask == Mask::All)
        );
    }

    #[test]
    fn a_grounded_strike_is_whole_body_until_the_legs_move() {
        let clips = Kind::Ranger.clips();
        let one = |_: &str| 1.;
        let mut playback = Playback::default();
        advance(
            &mut playback,
            clips,
            one,
            0.,
            0.,
            false,
            0.,
            0.05,
            false,
            false,
        );
        playback.strike("Sword_Attack", 1., 0.);
        for step in 1..5 {
            advance(
                &mut playback,
                clips,
                one,
                0.,
                0.,
                false,
                f64::from(step) * 0.05,
                0.05,
                false,
                false,
            );
        }
        let standing = advance(
            &mut playback,
            clips,
            one,
            0.,
            0.,
            false,
            0.25,
            0.05,
            false,
            false,
        );
        assert_eq!(standing.lower, ("Sword_Attack", 0.25));
        assert!(standing.layers.iter().all(|layer| layer.mask == Mask::All));
        // Stepping off hands the legs back to locomotion without restarting the swing.
        let moving = advance(
            &mut playback,
            clips,
            one,
            1.,
            0.,
            false,
            0.3,
            0.05,
            false,
            false,
        );
        assert_eq!(moving.lower.0, "Walk_Loop");
        assert_eq!(moving.upper, ("Sword_Attack", 0.3));
        let (upper, lower) = region_weights(&moving);
        assert!(
            (upper - 1.).abs() < 1e-4 && (lower - 1.).abs() < 1e-4,
            "{moving:?}"
        );
    }

    #[test]
    fn blend_durations_and_weights_shape_the_layers() {
        let clips = Kind::Ranger.clips();
        let one = |_: &str| 1.;
        let mut playback = Playback::default();
        let cues = |speed, now| Cues {
            speed,
            now,
            dt: 0.05,
            ..Cues::default()
        };
        let blend = Blend {
            lower_seconds: 0.2,
            upper_seconds: 0.1,
            lower_weight: 1.,
            upper_weight: 0.5,
        };
        playback.advance(clips, one, cues(0., 0.), blend);
        playback.advance(clips, one, cues(1., 0.05), blend);
        let half = playback.advance(clips, one, cues(1., 0.1), blend);
        let walk: f32 = half
            .layers
            .iter()
            .filter(|l| l.clip == "Walk_Loop")
            .map(|l| l.weight)
            .sum();
        assert!((walk - 0.25).abs() < 1e-4, "{half:?}");
        playback.strike("Sword_Attack", 1., 0.1);
        for step in 0..4 {
            playback.advance(clips, one, cues(1., 0.15 + f64::from(step) * 0.05), blend);
        }
        let tuned = playback.advance(clips, one, cues(1., 0.4), blend);
        let strike: f32 = tuned
            .layers
            .iter()
            .filter(|l| l.clip == "Sword_Attack")
            .map(|l| l.weight)
            .sum();
        assert!((strike - 0.5).abs() < 1e-4, "{tuned:?}");
        assert_eq!(
            Blend {
                lower_seconds: f32::NAN,
                upper_seconds: 3.,
                lower_weight: -1.,
                upper_weight: 0.4,
            }
            .clamped(),
            Blend {
                lower_seconds: 0.15,
                upper_seconds: 1.,
                lower_weight: 0.,
                upper_weight: 0.4,
            }
        );
    }

    #[test]
    fn strikes_use_the_class_and_ability_clips_the_bundle_has() {
        use mmorpg_core::ability::ids;
        assert_eq!(
            Kind::Ranger.strike(Some(PlayerClass::Warden), Some(ids::HEROIC_STRIKE)),
            "Sword_Attack"
        );
        assert_eq!(
            Kind::Ranger.strike(Some(PlayerClass::Warden), Some(ids::SHIELD_BASH)),
            "Shield_OneShot"
        );
        assert_eq!(
            Kind::Ranger.strike(Some(PlayerClass::Ranger), None),
            "Bow_Release"
        );
        assert_eq!(
            Kind::Ranger.strike(Some(PlayerClass::Arcanist), Some(ids::FIREBOLT)),
            "Spell_Simple_Shoot"
        );
        assert_eq!(Kind::Ranger.strike(None, None), "Sword_Attack");
        assert_eq!(Kind::Wolf.strike(None, Some(ids::SHIELD_BASH)), "Attack");
    }

    /// One frame of a unit in `water`, travelling at `speed` m/s straight ahead and `vertical`.
    fn swim_frame(
        playback: &mut Playback,
        duration: impl Fn(&str) -> f32,
        water: WaterMode,
        speed: f64,
        vertical: f64,
        now: f64,
    ) -> Animation {
        playback.advance(
            Kind::Ranger.clips(),
            duration,
            Cues {
                speed,
                vertical_speed: vertical,
                water,
                now,
                dt: 0.1,
                ..Cues::default()
            },
            Blend::default(),
        )
    }

    #[test]
    fn the_zones_water_mode_picks_wade_tread_stroke_dive_and_surface_clips() {
        let one = |_: &str| 1.;
        let mut playback = Playback::default();
        let mut now = 0.;
        let mut frame = |water, speed, vertical| {
            now += 0.1;
            swim_frame(&mut playback, one, water, speed, vertical, now)
        };
        assert_eq!(frame(WaterMode::Dry, 1., 0.).lower.0, "Walk_Loop");
        assert_eq!(frame(WaterMode::Wading, 1., 0.).lower.0, "Wade_Fwd_Loop");
        // Reaching swimming depth plays the entry over the stroke, then the stroke alone.
        let entering = frame(WaterMode::Swimming, 1., 0.);
        assert_eq!(entering.lower.0, "Swim_Enter");
        let mut swimming = entering;
        for _ in 0..12 {
            swimming = frame(WaterMode::Swimming, 1., 0.);
        }
        assert_eq!(swimming.lower.0, "Swim_Fwd_Loop");
        // Surface swimming never jumps or falls on vertical motion; it treads in place.
        assert_eq!(frame(WaterMode::Swimming, 0., 0.).lower.0, "Swim_Idle_Loop");
        assert_eq!(
            frame(WaterMode::Submerged, 0., -1.).lower.0,
            "Swim_Down_Loop"
        );
        assert_eq!(frame(WaterMode::Submerged, 0.5, 1.).lower.0, "Swim_Up_Loop");
        // Leaving swimming depth plays the exit, then the ordinary locomotion.
        assert_eq!(frame(WaterMode::Wading, 0., 0.).lower.0, "Swim_Exit");
        let mut ashore = frame(WaterMode::Dry, 0., 0.);
        for _ in 0..12 {
            ashore = frame(WaterMode::Dry, 0., 0.);
        }
        assert_eq!(ashore.lower.0, "Idle_Loop");
        assert!(ashore.attitude.pitch.abs() < 0.01 && ashore.attitude.heading.abs() < 0.01);
    }

    #[test]
    fn a_bundle_without_swim_clips_falls_back_in_place_to_its_land_clips() {
        let land = |clip: &str| {
            if clip.starts_with("Swim_") || clip.starts_with("Wade_") {
                0.
            } else {
                1.
            }
        };
        let mut playback = Playback::default();
        let mut now = 0.;
        let mut frame = |water, speed, vertical| {
            now += 0.1;
            swim_frame(&mut playback, land, water, speed, vertical, now)
        };
        assert_eq!(frame(WaterMode::Wading, 1., 0.).lower.0, "Walk_Loop");
        // No entry clip: straight to the leaned walk, never a fall or landing.
        let stroke = frame(WaterMode::Swimming, 1.5, -2.);
        assert_eq!(stroke.lower.0, "Walk_Loop");
        assert!(
            stroke
                .layers
                .iter()
                .all(|layer| !layer.clip.starts_with("Jump_"))
        );
        assert!(stroke.attitude.pitch > 0.);
        assert_eq!(frame(WaterMode::Submerged, 0., 0.).lower.0, "Idle_Loop");
        assert_eq!(frame(WaterMode::Submerged, 0., -1.).lower.0, "Walk_Loop");
        assert_eq!(frame(WaterMode::Dry, 0., 0.).lower.0, "Idle_Loop");
    }

    #[test]
    fn a_swimmer_leans_into_its_stroke_and_stands_again_ashore() {
        let one = |_: &str| 1.;
        let mut playback = Playback::default();
        let mut now = 0.;
        let mut pitch = 0.;
        for _ in 0..20 {
            now += 0.1;
            let animation = swim_frame(&mut playback, one, WaterMode::Swimming, 2., 0., now);
            assert!(animation.attitude.pitch >= pitch);
            pitch = animation.attitude.pitch;
        }
        assert!((pitch - super::super::swim::SURFACE_PITCH).abs() < 0.01);
        // Climbing out eases back upright rather than snapping.
        now += 0.1;
        let out = swim_frame(&mut playback, one, WaterMode::Dry, 2., 0., now);
        assert!(out.attitude.pitch > 0. && out.attitude.pitch < pitch);
        // A strike while treading stays on the upper body: no footwork without footing.
        playback.strike("Sword_Attack", 1., now);
        now += 0.1;
        swim_frame(&mut playback, one, WaterMode::Swimming, 0., 0., now);
        now += 0.1;
        let struck = swim_frame(&mut playback, one, WaterMode::Swimming, 0., 0., now);
        assert_eq!(struck.upper.0, "Sword_Attack");
        assert_ne!(struck.lower.0, "Sword_Attack");
    }

    fn roll_frame(playback: &mut Playback, rolling: bool, direction: f64, now: f64) -> Animation {
        playback.advance(
            Kind::Ranger.clips(),
            |clip: &str| if clip == "Roll" { 1.4667 } else { 1. },
            Cues {
                speed: 5.85,
                direction,
                rolling,
                now,
                dt: 0.1,
                ..Cues::default()
            },
            Blend::default(),
        )
    }

    #[test]
    fn a_roll_plays_its_clip_once_over_the_roll_turned_toward_its_travel() {
        let mut playback = Playback::default();
        let back = std::f64::consts::PI;
        let first = roll_frame(&mut playback, true, back, 0.1);
        assert_eq!(first.lower.0, "Roll");
        assert!((first.attitude.heading.abs() - back).abs() < 1e-9);
        // The clip plays at the pace that fits it into the 0.6 s roll.
        let mut last = first;
        for frame in 2..=6 {
            last = roll_frame(&mut playback, true, back, f64::from(frame) / 10.);
        }
        let expected = 0.5 * 1.4667 / ROLL_SECONDS;
        assert!(
            (f64::from(last.lower.1) - expected).abs() < 0.01,
            "{:?}",
            last.lower
        );
        // After the roll the run takes over and the body eases back toward its facing.
        let after = roll_frame(&mut playback, false, 0., 0.7);
        assert_eq!(after.lower.0, "Jog_Fwd_Loop");
        assert!(after.attitude.heading.abs() < back);
        // A second roll starts its clip again.
        let again = roll_frame(&mut playback, true, 0., 0.8);
        assert_eq!(again.lower, ("Roll", 0.));
    }

    fn climb_frame(
        playback: &mut Playback,
        duration: impl Fn(&str) -> f32,
        traversal: TraversalView,
        (speed, direction, vertical): (f64, f64, f64),
        now: f64,
    ) -> Animation {
        playback.advance(
            Kind::Ranger.clips(),
            duration,
            Cues {
                traversal,
                speed,
                direction,
                vertical_speed: vertical,
                now,
                dt: 0.05,
                ..Cues::default()
            },
            Blend::default(),
        )
    }

    #[test]
    fn the_zones_climb_and_mantle_pick_the_traversal_clips() {
        let one = |_: &str| 1.;
        let mut playback = Playback::default();
        let mut now = 0.;
        let mut frame = |traversal, motion| {
            now += 0.05;
            climb_frame(&mut playback, one, traversal, motion, now)
        };
        use TraversalView::{Climbing, ClimbingLadder, ClimbingRope, Free, MantlingHigh};
        let still = (0., 0., 0.);
        assert_eq!(frame(Climbing, still).lower.0, "Climb_Idle_Loop");
        // Up and down along the face at the zone's 0.9 m/s play at the authored rate; a climber
        // rising never jumps.
        let up = frame(Climbing, (0., 0., 0.9));
        assert_eq!(up.lower.0, "Climb_Up_Loop");
        assert_eq!(frame(Climbing, (0., 0., -0.9)).lower.0, "Climb_Down_Loop");
        // Strafing along the face: travel to the right is a negative yaw from the facing.
        let right = (0.75, -FRAC_PI_2, 0.);
        assert_eq!(frame(Climbing, right).lower.0, "Climb_Right_Loop");
        assert_eq!(
            frame(Climbing, (0.75, FRAC_PI_2, 0.)).lower.0,
            "Climb_Left_Loop"
        );
        assert_eq!(
            frame(ClimbingLadder, (0., 0., 0.6)).lower.0,
            "Ladder_Up_Loop"
        );
        assert_eq!(frame(ClimbingLadder, right).lower.0, "Ladder_Idle_Loop");
        assert_eq!(
            frame(ClimbingRope, (0., 0., -0.6)).lower.0,
            "Rope_Down_Loop"
        );
        // Climbing over the top tops out, whatever the ledge.
        assert_eq!(frame(MantlingHigh, (0., 0., 3.)).lower.0, "Climb_TopOut");
        // Back on the ground, a mantle from it plays by the ledge's height.
        for _ in 0..40 {
            frame(Free, still);
        }
        assert_eq!(
            frame(TraversalView::MantlingLow, (0., 0., 3.)).lower.0,
            "Mantle_Low"
        );
        for _ in 0..40 {
            frame(Free, still);
        }
        assert_eq!(
            frame(TraversalView::MantlingMid, (0., 0., 3.)).lower.0,
            "Mantle_Mid"
        );
        for _ in 0..40 {
            frame(Free, still);
        }
        assert_eq!(frame(MantlingHigh, (0., 0., 3.)).lower.0, "Mantle_High");
        // Letting go of a face plays the let-go over the fall.
        frame(Climbing, still);
        let falling = frame(Free, (0., 0., -2.));
        assert_eq!(falling.lower.0, "Climb_LetGo");
    }

    #[test]
    fn climb_loops_keep_pace_with_the_zone_over_their_authored_speed() {
        let motions = super::super::traversal::Motions::parse(
            r#"{"schema": "humanoid-clips-v1", "clips": [
                {"name": "Climb_Up_Loop", "climbSpeedMetersPerSecond": 0.45}
            ]}"#,
        )
        .expect("valid descriptor");
        let one = |_: &str| 1.;
        let clips = Kind::Ranger.clips();
        let available = |clip: &str| one(clip) > 0.;
        let cues = |motions| Cues {
            traversal: TraversalView::Climbing,
            motions,
            vertical_speed: 0.9,
            ..Cues::default()
        };
        let authored = Playback::climb(clips, &available, cues(Some(&motions)));
        assert_eq!(
            (authored.clip, authored.rate),
            ("Climb_Up_Loop", 1.6_f32.min(2.))
        );
        let fallback = Playback::climb(clips, &available, cues(None));
        assert!((fallback.rate - 1.).abs() < 1e-6);
        // A bundle without the climb clips keeps the earlier in-place walk.
        let missing = |clip: &str| !clip.starts_with("Climb_");
        assert_eq!(
            Playback::climb(clips, &missing, cues(None)).clip,
            "Walk_Loop"
        );
    }
}
