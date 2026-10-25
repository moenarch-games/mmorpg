//! Which imported model presents a unit, its clip names and the per-unit clip clock.
//!
//! Players use asset-tooling's Ranger humanoid (Atlas 6.3.7); wolves, vermin and boars use the
//! bundled Quaternius CC0 animals (Atlas 6.3.6); the named Greyhaven residents wear the Ranger in
//! their appearance presets (#418) where the client draws them; humanoid creatures and other
//! NPCs keep their procedural models. Each client imports and skins the models with its own
//! provider; this module only decides what to pose. Presentation only, never read by gameplay.
use mmorpg_core::{
    AbilityId, CreatureFamily, CreatureTemplateId, EntityRef, EntitySnapshot, Equipment,
    EquipmentSlot, PlayerClass, ZoneContent,
};

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

/// A named Greyhaven resident drawn in the Ranger bundle's `humanoid-appearance` preset (#418).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Resident {
    Marshal,
    Guard,
    Innkeeper,
    Farmer,
    Healer,
}

impl Resident {
    /// The resident an NPC of this canonical name is, if it has a preset. Tanner Hilda Brook has
    /// none yet (the resident outfits exist on the male figure only).
    #[must_use]
    pub fn of_name(name: &str) -> Option<Self> {
        match name {
            "Marshal Elden Greywatch" => Some(Self::Marshal),
            "Greyhaven Guard" => Some(Self::Guard),
            "Innkeeper Bram Tolliver" => Some(Self::Innkeeper),
            "Farmer Osric Mill" => Some(Self::Farmer),
            "Brother Aldous" => Some(Self::Healer),
            _ => None,
        }
    }

    /// The bundle's `humanoid-appearance.presets` id.
    #[must_use]
    pub const fn preset(self) -> &'static str {
        match self {
            Self::Marshal => "resident.marshal",
            Self::Guard => "resident.guard",
            Self::Innkeeper => "resident.innkeeper",
            Self::Farmer => "resident.farmer",
            Self::Healer => "resident.healer",
        }
    }

    /// What the resident carries: the guards keep their sword.
    #[must_use]
    pub const fn loadout(self) -> Option<Loadout> {
        match self {
            Self::Guard => Some(Loadout::Sword),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
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
    /// Class- and ability-specific strikes, for a model that has them (#384).
    pub strikes: Option<StrikeClips>,
    /// The joint whose subtree is the upper body, for a model that layers its upper body over
    /// its legs (Atlas 6.6.45); every other joint (root, pelvis, legs) is the lower body.
    pub upper_body: Option<&'static str>,
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

impl Kind {
    pub const ALL: [Self; 4] = [Self::Ranger, Self::Wolf, Self::Rat, Self::Pig];

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
        Self::ALL.into_iter().find(|kind| kind.name() == name)
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
                strikes: Some(StrikeClips {
                    shield_bash: "Shield_OneShot",
                    bow: "Bow_Release",
                    spell: "Spell_Simple_Shoot",
                }),
                upper_body: Some(UPPER_BODY_ROOT),
            },
            Self::Wolf => Clips {
                idle: "Idle",
                walk: "Walk",
                run: "Gallop",
                attack: "Attack",
                death: "Death",
                guard: None,
                jump: None,
                strikes: None,
                upper_body: None,
            },
            Self::Rat => Clips {
                idle: "Rat_Idle",
                walk: "Rat_Walk",
                run: "Rat_Run",
                attack: "Rat_Attack",
                death: "Rat_Death",
                guard: None,
                jump: None,
                strikes: None,
                upper_body: None,
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
                strikes: None,
                upper_body: None,
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
}

/// What the projection shows of one unit this frame. Every cue is zone-accepted state or the
/// projected motion; the clip clocks never feed back into it.
#[derive(Clone, Copy, Debug, Default)]
pub struct Cues {
    /// Horizontal speed in metres per second.
    pub speed: f64,
    /// Cosmetic vertical velocity in metres per second, zero while swimming.
    pub vertical_speed: f64,
    pub dead: bool,
    /// The zone's guard protects now: a ready, grounded raised shield (viewer only).
    pub guarding: bool,
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

/// An accepted strike being presented.
#[derive(Clone, Copy, Debug)]
struct Strike {
    clip: &'static str,
    serial: u32,
    started: f64,
    until: f64,
    /// Begun standing on the ground: the authored footwork plays on the legs too (Atlas 6.6.2),
    /// until the unit moves or leaves the ground; then only the upper body carries it.
    full_body: bool,
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
    /// The local player's wind-up, until the zone answers (Atlas 6.6.10).
    preparation: Option<Preparation>,
    guarding: bool,
    serial: u32,
    landing: u32,
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
    pub fn strike(&mut self, clip: &'static str, duration: f32, now: f64) {
        self.serial = self.serial.wrapping_add(1);
        self.strike = Some(Strike {
            clip,
            serial: self.serial,
            started: now,
            until: now + f64::from(duration.max(0.3)),
            full_body: true,
        });
    }

    /// The zone accepted the prepared strike: it continues from the wind-up's phase when the
    /// wind-up shows the same clip, so the swing commits without restarting. Otherwise it is an
    /// ordinary [`Self::strike`] and the wind-up ends.
    pub fn commit(&mut self, clip: &'static str, duration: f32, now: f64) {
        match self.preparation.take() {
            Some(preparation) if preparation.clip == clip => {
                let started = now - f64::from(preparation.at(now, duration));
                self.strike = Some(Strike {
                    clip,
                    serial: preparation.serial,
                    started,
                    until: started + f64::from(duration.max(0.3)),
                    full_body: true,
                });
            }
            _ => self.strike(clip, duration, now),
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

    /// Advances the clocks and returns the layers to pose this frame; `duration` gives a clip's
    /// length in seconds (zero when the model lacks it).
    pub fn advance(
        &mut self,
        clips: Clips,
        duration: impl Fn(&str) -> f32,
        cues: Cues,
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
        // The projection already supplies cosmetic vertical velocity. Retain the flight phase
        // through the apex instead of treating its zero velocity as a landing. This is a
        // visual cue, not a grounded/physics decision.
        self.flight = if dead {
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
            self.preparation = None;
        }
        self.guarding = guarding;
        self.strike = self.strike.filter(|strike| now < strike.until);
        let standing = matches!(self.flight, Flight::Ground) && speed <= 0.25;
        let layered = clips.upper_body.is_some();
        let mut split = false;
        if let Some(strike) = &mut self.strike {
            let full_body = strike.full_body && (standing || !layered);
            split = strike.full_body && !full_body;
            strike.full_body = full_body;
        }
        let strike = self.strike.filter(|_| !reduced).map(|strike| {
            #[allow(clippy::cast_possible_truncation)]
            let at = (now - strike.started).max(0.) as f32;
            (
                strike.full_body,
                Want {
                    at: Some(at),
                    ..Want::hold(strike.clip, strike.serial)
                },
            )
        });
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
        let ground = if speed > 3.2 {
            #[allow(clippy::cast_possible_truncation)]
            Want::looped(clips.run, (speed / 5.).clamp(0.7, 1.5) as f32)
        } else if speed > 0.25 {
            #[allow(clippy::cast_possible_truncation)]
            Want::looped(clips.walk, (speed / 1.8).clamp(0.6, 1.6) as f32)
        } else {
            // A standing protecting guard holds the shield up with its own stance.
            Want::looped(clips.guard.filter(|_| guarding).unwrap_or(clips.idle), 1.)
        };
        let base = if dead {
            Want::hold(clips.death, 0)
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
        let lower = if dead {
            None
        } else if let Some((true, want)) = strike {
            Some(want)
        } else if let (Some(jump), Flight::Landing) = (clips.jump, self.flight)
            && !reduced
            && available(jump.land)
        {
            Some(Want::hold(jump.land, self.landing))
        } else {
            None
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
        // The first clip a unit shows has nothing to blend from.
        let seconds = if self.base.current.is_some() {
            blend.lower_seconds
        } else {
            0.
        };
        self.base.advance(Some(base), dt, seconds, reduced);
        self.lower.advance(lower, dt, blend.lower_seconds, reduced);
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
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mmorpg_core::{CreatureId, NpcId, greyhaven_vale};

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
                None,
                Some("resident.innkeeper"),
                Some("resident.farmer"),
                Some("resident.healer"),
                Some("resident.guard"),
                Some("resident.guard"),
                Some("resident.guard"),
                Some("resident.guard"),
            ]
        );
        let names: Vec<_> = PlayerClass::ALL
            .into_iter()
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
    fn classes_wear_their_starter_outfit_and_drops_replace_its_slot_pieces() {
        let presets: Vec<_> = PlayerClass::ALL
            .into_iter()
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
}
