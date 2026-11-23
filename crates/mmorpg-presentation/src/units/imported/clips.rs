//! Clip tables: each kind's clip names, and the clips a strike, cast, reaction or pose picks.

use mmorpg_core::{
    AbilityId, CreatureFamily, CreatureTemplateId, EntityRef, EntitySnapshot, Ledge, PlayerClass,
    StrikeAim, StrikeHeight, StrikeVariant, WeaponFamily, ZoneContent,
};
use strum::VariantArray;

use super::*;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, strum::VariantArray)]
pub enum Kind {
    Ranger,
    // The wildlife release's beasts (#889).
    Wolf,
    Rat,
    Boar,
    Bear,
    Spider,
    Lurker,
    /// The forest lynx (#989).
    Lynx,
    /// The marsh adder (#990).
    Adder,
    /// The cave bat (#995).
    Bat,
    /// The river crayfish (#996).
    Crayfish,
    /// The Southwood stag (#997).
    Stag,
}

/// Clip names of one model.
#[derive(Clone, Copy, Debug)]
pub struct Clips {
    pub idle: &'static str,
    pub walk: &'static str,
    pub run: &'static str,
    pub attack: &'static str,
    pub death: &'static str,
    /// Deaths falling back, forward and spinning (asset-tooling #711), varied per death when
    /// the bundle has them (#885); otherwise `death`.
    pub deaths: Option<[&'static str; 3]>,
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
    /// The Warden's sword-and-shield clips and the Redbrand specials, for a model that has them
    /// (#582, asset-tooling #447).
    pub sword: Option<SwordClips>,
    /// The Rogue's dagger, throw, flourish and bandage clips, for a model that has them (#603,
    /// asset-tooling #418).
    pub dagger: Option<DaggerClips>,
    /// The upper-body bow cycle, the rapid release and the power draw, for a model that has
    /// them (#607, asset-tooling #419).
    pub bow: Option<BowClips>,
    /// The crossbow clips, for a model that has them (#607, asset-tooling #419).
    pub crossbow: Option<CrossbowClips>,
    /// The casters' cast styles, staff and mace swings, war cry and interrupt, for a model that
    /// has them (#609, asset-tooling #420).
    pub caster: Option<CasterClips>,
    /// The emote, door, loot, talk, trade, sitting and bed clips, for a model that has them
    /// (#619, asset-tooling #421).
    pub social: Option<SocialClips>,
    /// What a creature holds still and out of a fight instead of `idle`, for a model that has
    /// it: the cave bat's `Perch` at its roost (#995), where it hovers in `idle` in a fight, and
    /// the stag's `Eat` (#997).
    pub rest: Option<&'static str>,
    /// A beast's two attacks and its alert (#889), for a model that has them.
    pub bites: Option<BiteClips>,
}

/// A beast's two attacks and its alert (#889), for a model that has them.
#[derive(Clone, Copy, Debug)]
pub struct BiteClips {
    /// By [`mmorpg_core::CreatureAttack::code`]: `Attack_A` quick, `Attack_B` heavy.
    pub attacks: [&'static str; 2],
    /// Played once as it turns to fight.
    pub alert: &'static str,
    /// What follows an attack, in order, for a model whose attack is a wind-up clip (#989): the
    /// lynx's `Pounce_Lunge`, whose contact is the zone's, then its `Pounce_Recover`; a
    /// strike clip that holds its own recovery, like the stag's `Attack_A` (#997), has none.
    pub follow: Option<(&'static str, Option<&'static str>)>,
}

/// The wildlife release's clip set (asset-tooling #716, #723): every hostile beast runs, bites
/// twice over, flinches, dies and turns alert.
const WILD_CLIPS: Clips = Clips {
    idle: "Idle",
    walk: "Walk",
    run: "Run",
    attack: "Attack_A",
    death: "Death",
    deaths: None,
    guard: None,
    jump: None,
    swim: None,
    roll: None,
    climb: None,
    strikes: None,
    upper_body: None,
    gaits: None,
    reactions: Some(ReactionClips {
        chest: "Hit",
        head: "Hit",
        knockback: "Hit",
        shove: "Hit",
        block: "Hit",
        guard_break: "Hit",
        light: ["Hit", "Hit"],
        heavy: "Hit",
        rooted: None,
    }),
    sword: None,
    dagger: None,
    bow: None,
    crossbow: None,
    caster: None,
    social: None,
    rest: None,
    bites: Some(BiteClips {
        attacks: ["Attack_A", "Attack_B"],
        alert: "Alert",
        follow: None,
    }),
};

/// The wildlife release's lynx (#989, asset-tooling #778): it rests and walks like the other
/// beasts, crouches to stalk as its fight starts, and pounces in three clips, `Pounce_Windup`,
/// `Pounce_Lunge` (whose contact the zone's pounce lands on) and `Pounce_Recover`.
const LYNX_CLIPS: Clips = Clips {
    attack: "Pounce_Lunge",
    bites: Some(BiteClips {
        attacks: ["Pounce_Windup", "Pounce_Windup"],
        alert: "Crouch_Stalk_Loop",
        follow: Some(("Pounce_Lunge", Some("Pounce_Recover"))),
    }),
    ..WILD_CLIPS
};

/// The wildlife release's marsh adder (#990, asset-tooling #778): it rests coiled and
/// slithers, hisses as its fight starts, and strikes in three clips: `Warn_Hiss_Loop` once
/// through (the zone's warning), `Strike` (whose contact the zone's strike lands on) and
/// `Strike_Recover`.
const ADDER_CLIPS: Clips = Clips {
    idle: "Idle_Coil_Loop",
    walk: "Slither_Loop",
    run: "Slither_Loop",
    attack: "Strike",
    bites: Some(BiteClips {
        attacks: ["Warn_Hiss_Loop", "Warn_Hiss_Loop"],
        alert: "Warn_Hiss_Loop",
        follow: Some(("Strike", Some("Strike_Recover"))),
    }),
    ..WILD_CLIPS
};

/// The wildlife release's cave bat (#995, asset-tooling #784): it hangs in its `Perch` at the
/// roost, drops off in `Launch` as its fight starts, hovers in `Idle` and flies in `Fly`, and
/// dives in three clips: `Dive_Windup` (the zone's wind-up), `Dive_Contact` (whose contact the
/// zone's dive lands on) and `Dive_Recover`.
const BAT_CLIPS: Clips = Clips {
    run: "Fly",
    walk: "Fly",
    attack: "Dive_Contact",
    rest: Some("Perch"),
    bites: Some(BiteClips {
        attacks: ["Dive_Windup", "Dive_Windup"],
        alert: "Launch",
        follow: Some(("Dive_Contact", Some("Dive_Recover"))),
    }),
    ..WILD_CLIPS
};

/// The wildlife release's stag (#997, asset-tooling #804): it grazes in `Eat` while it stands out of a
/// fight and walks like the ambient stag, throws its head up in `Alert` as its fight starts, and charges in two clips:
/// `Alert` once through (the zone's warning), then `Attack_A`, the lowered-antler charge whose
/// contact the zone's lands on and whose tail is its blown recovery. The rearing `Attack_B`
/// goes unused.
const STAG_CLIPS: Clips = Clips {
    rest: Some("Eat"),
    bites: Some(BiteClips {
        attacks: ["Alert", "Alert"],
        alert: "Alert",
        follow: Some(("Attack_A", None)),
    }),
    ..WILD_CLIPS
};

/// The authored ranged clips beside the full-body bow cycle (#607, asset-tooling #419), every
/// one on the upper body only. Each one the bundle lacks falls back to what played before.
#[derive(Clone, Copy, Debug)]
pub struct BowClips {
    /// The bow cycle re-authored with the hips square: it replaces the full-body cycle
    /// whenever the cycle plays on the upper body alone, over a gait or the idle legs.
    pub upper: CastClips,
    /// The upper body's release ([`StrikeClips::bow`]'s twin).
    pub release: &'static str,
    /// One shot from the set string and back under Rapid Fire's haste: draw, loose, nock.
    pub rapid: &'static str,
    /// Aimed Shot's and Piercing Shot's slow draw past full draw, held at its end; it looses
    /// with [`Self::release`].
    pub power_draw: &'static str,
}

impl BowClips {
    /// Whether `clip` is one of them.
    #[must_use]
    pub fn contains(self, clip: &str) -> bool {
        self.upper.contains(clip) || [self.release, self.rapid, self.power_draw].contains(&clip)
    }

    /// The upper-body twin of a clip of the full-body cycle `full` or its `release`.
    #[must_use]
    pub fn upper_of(self, full: CastClips, release: &str, clip: &str) -> Option<&'static str> {
        let upper = self.upper;
        if clip == release {
            Some(self.release)
        } else if Some(clip) == full.aim {
            upper.aim
        } else if clip == full.enter {
            Some(upper.enter)
        } else if clip == full.hold {
            Some(upper.hold)
        } else if Some(clip) == full.recovery {
            upper.recovery
        } else if clip == full.cancel {
            Some(upper.cancel)
        } else {
            None
        }
    }
}

/// The Redbrand Crossbowman's crossbow clips (#607, asset-tooling #419), every one on the upper
/// body only.
#[derive(Clone, Copy, Debug)]
pub struct CrossbowClips {
    /// Carried low in both hands: out of combat and on the move.
    pub idle: &'static str,
    /// The butt at the shoulder, loaded: standing in combat, and through Crossbow Bolt's cast.
    pub aim: &'static str,
    /// The shot, looses at the clip's `crossbow.releaseSeconds` …
    pub fire: &'static str,
    /// … and the reload after it, back to the aim.
    pub reload: &'static str,
}

impl CrossbowClips {
    /// Whether `clip` is one of them.
    #[must_use]
    pub fn contains(self, clip: &str) -> bool {
        [self.idle, self.aim, self.fire, self.reload].contains(&clip)
    }

    /// The cycle a crossbowman's cast holds (#607): the aim through the cast, the reload after
    /// the shot. It has no let-down of its own; a cancel holds the aim a moment.
    #[must_use]
    pub const fn cycle(self) -> CastClips {
        CastClips {
            aim: None,
            enter: self.aim,
            hold: self.aim,
            recovery: Some(self.reload),
            cancel: self.aim,
            footwork: false,
        }
    }
}

/// The casters' shared cast styles, staff and mace swings, war cry and interrupt (#609,
/// asset-tooling #420). The school (fire, frost, shadow, holy, nature) shows through VFX, not
/// the clip. All but [`Self::nova`] and [`Self::summon_raise`] play on the upper body only.
/// Each one the bundle lacks falls back to what played before (the `Spell_Simple_*` hold and
/// release).
#[derive(Clone, Copy, Debug)]
pub struct CasterClips {
    /// Held through a cast bar, looped.
    pub charge: &'static str,
    /// A cast's release from the charge: the palm push of a bolt …
    pub push: &'static str,
    /// … or the overhand throw of a blast.
    pub overhand: &'static str,
    /// Channel loops: the staff's beam, the palm's drain, the staff raised to the sky.
    pub beam: &'static str,
    pub drain: &'static str,
    pub sky: &'static str,
    /// The staff's foot stamped for a burst round the caster, whole body.
    pub nova: &'static str,
    /// The instants: a cone swept ahead, a pointed curse, a sweep over the ground, a ward on
    /// oneself, a touch for an ally, and the Warden's war cry.
    pub cone: &'static str,
    pub curse: &'static str,
    pub ground: &'static str,
    pub ward: &'static str,
    pub ally: &'static str,
    pub warcry: &'static str,
    /// Raise Skeleton's kneel, whole body: the clip is the cast, from its start.
    pub summon_raise: &'static str,
    /// Call Companion's whistle: it ends with the cast, after the charge.
    pub summon_call: &'static str,
    /// The staff classes' alternating auto-attack: a jab and a sweep, two-handed.
    pub staff: [&'static str; 2],
    /// The Cleric's alternating auto-attack: an overhand and a backhand blow.
    pub mace: [&'static str; 2],
    /// Raising summoned terrain (#293); no ability raises any yet.
    pub wall: &'static str,
    /// A cast or channel broken off, from the charge.
    pub interrupted: &'static str,
}

impl CasterClips {
    /// Whether `clip` is one of them.
    #[must_use]
    pub fn contains(self, clip: &str) -> bool {
        [
            self.charge,
            self.push,
            self.overhand,
            self.beam,
            self.drain,
            self.sky,
            self.nova,
            self.cone,
            self.curse,
            self.ground,
            self.ward,
            self.ally,
            self.warcry,
            self.summon_raise,
            self.summon_call,
            self.staff[0],
            self.staff[1],
            self.mace[0],
            self.mace[1],
            self.wall,
            self.interrupted,
        ]
        .contains(&clip)
    }

    /// Which body `clip` drives: the nova and the summoning kneel the whole body standing,
    /// every other one only the upper body; `None` for a clip not among these.
    #[must_use]
    pub fn region(self, clip: &str) -> Option<Region> {
        if clip == self.nova || clip == self.summon_raise {
            Some(Region::Footwork)
        } else {
            self.contains(clip).then_some(Region::Upper)
        }
    }
}

/// How a caster shows a cast or channel of an ability (#609).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CastStyle {
    /// The charge, looped through the cast bar; the release plays when the cast completes.
    Charge(&'static str),
    /// A channel loop, looped while channelling.
    Channel(&'static str),
    /// A one-shot that is the whole cast, played from its start (Raise Skeleton).
    Whole(&'static str),
    /// The charge, then a one-shot timed to end with the cast (Call Companion): `(charge,
    /// finish)`.
    Finish(&'static str, &'static str),
}

impl CastStyle {
    /// The clip the cast shows first.
    #[must_use]
    pub const fn first(self) -> &'static str {
        match self {
            Self::Charge(clip)
            | Self::Channel(clip)
            | Self::Whole(clip)
            | Self::Finish(clip, _) => clip,
        }
    }

    /// The clip a style that plays its own release shows it with: then the zone's resolution
    /// plays nothing more.
    #[must_use]
    pub const fn released(self) -> Option<&'static str> {
        match self {
            Self::Whole(clip) | Self::Finish(_, clip) => Some(clip),
            Self::Charge(_) | Self::Channel(_) => None,
        }
    }
}

/// Where a spell's effect starts in a caster clip (`cast.channelJoint`, `cast.channelPoint`,
/// asset-tooling #420): `point` metres in `hand`'s glTF frame, the palm's middle or the staff's
/// head (on the nova, its foot). Spawn a bolt or anchor a beam or drain here.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Channel {
    pub hand: Hand,
    pub point: [f64; 3],
}

/// The authored dagger, throw, flourish and bandage clips (#603, asset-tooling #418), every one
/// on the upper body only. Each one the bundle lacks falls back to what played before.
#[derive(Clone, Copy, Debug)]
pub struct DaggerClips {
    /// The low twin-dagger stance, looped while a dagger holder stands.
    pub idle: &'static str,
    /// The auto-attack: the right dagger's stab, alternating with the left dagger's slash when
    /// both hands hold one.
    pub stab: &'static str,
    pub slash: &'static str,
    /// Sinister Strike's double stab, Poisoned Blade's coat and slash, Gouge's pommel jab.
    pub sinister: &'static str,
    pub poison: &'static str,
    pub gouge: &'static str,
    /// The haste flourish: Adrenaline Rush and the Ranger's Rapid Fire.
    pub flourish: &'static str,
    /// The sidearm knife throw and the underarm flask or bomb lob, drawn from the belt.
    pub throw_knife: &'static str,
    pub throw_lob: &'static str,
    /// Wrapping a bandage round the forearm: Crude Bandage.
    pub bandage: &'static str,
}

impl DaggerClips {
    /// Whether `clip` is one of them; each plays on the upper body.
    #[must_use]
    pub fn contains(self, clip: &str) -> bool {
        [
            self.idle,
            self.stab,
            self.slash,
            self.sinister,
            self.poison,
            self.gouge,
            self.flourish,
            self.throw_knife,
            self.throw_lob,
            self.bandage,
        ]
        .contains(&clip)
    }
}

/// The authored sword-and-shield clips (#582, asset-tooling #447). Each one the bundle lacks
/// falls back to what played before: the sword attack, the shield idle, the idle or nothing.
#[derive(Clone, Copy, Debug)]
pub struct SwordClips {
    /// `Sword_Slash_{Low,Mid,High}_{L,C,R}` by [`StrikeHeight::code`] and [`mmorpg_core::StrikeSide::code`]:
    /// the upper body's slash toward an aim, to the figure's own left, centre or right.
    pub slashes: [[&'static str; 3]; 3],
    /// The undirected Heroic Strike, on the upper body.
    pub heroic: &'static str,
    /// Cleave's turning sweep, with footwork.
    pub cleave: &'static str,
    /// Charge's rush, looped while the zone carries the charger, and its shield impact on arrival.
    pub charge_run: &'static str,
    pub charge_impact: &'static str,
    /// The raised shield in the low, middle and high guard lane by [`StrikeHeight::code`], on
    /// the upper body.
    pub guards: [&'static str; 3],
    /// Reeling, looped while stunned.
    pub stun: &'static str,
    /// Rising from a kneel, once, back from death.
    pub respawn: &'static str,
    /// Garrick Redbrand's whirl and the Redbrand Brute's overhead smash, whole body and in place.
    pub whirl: &'static str,
    pub smash: &'static str,
    /// The skirmisher's footwork (asset-tooling#757), whole body and in place, from the sword
    /// guard back to it: a sidestep to the figure's left and right, and a hop back. The Redbrand
    /// Bandit flanks and falls back with them (#963).
    pub sidesteps: [&'static str; 2],
    pub hop_back: &'static str,
    /// The Redbrand shield-bearer's guard (#953, asset-tooling #774), whole body: the round
    /// shield held up while it fights, its planted 45° pivots to the figure's left and right,
    /// the shield taking a blocked blow, and the stagger when a blow gets past it.
    pub shield_guard: &'static str,
    pub shield_turns: [&'static str; 2],
    pub shield_impact: &'static str,
    pub shield_stagger: &'static str,
}

impl SwordClips {
    /// The slash toward `aim`.
    #[must_use]
    pub const fn slash(self, aim: StrikeAim) -> &'static str {
        self.slashes[aim.height.code() as usize][aim.side.code() as usize]
    }

    /// The raised shield in `lane`.
    #[must_use]
    pub const fn guard(self, lane: StrikeHeight) -> &'static str {
        self.guards[lane.code() as usize]
    }

    /// Whether `clip` is one of the slashes.
    #[must_use]
    pub fn is_slash(self, clip: &str) -> bool {
        self.slashes.iter().flatten().any(|&slash| slash == clip)
    }

    /// Which body `clip` drives as a strike: the slashes and the heroic strike only the upper
    /// body, the whirl, the smash and the charge impact always the whole body, the cleave and
    /// the rise with footwork; `None` for a clip not among these.
    #[must_use]
    pub fn region(self, clip: &str) -> Option<Region> {
        if self.is_slash(clip) || clip == self.heroic {
            Some(Region::Upper)
        } else if clip == self.whirl || clip == self.smash || clip == self.charge_impact {
            Some(Region::Whole)
        } else if clip == self.cleave || clip == self.respawn {
            Some(Region::Footwork)
        } else {
            None
        }
    }
}

/// The heavy run's legs (#1073, asset-tooling #823): a shorter, braced stride at the run's
/// 6.3 m/s, played under [`CARRY_RUN`] while a load is carried at run speed.
pub const HEAVY_RUN: &str = "Heavy_Run_Fwd_Loop";
/// The upper body hugging a `load.*` item over [`HEAVY_RUN`] (#1073); it has no legs of its own.
pub const CARRY_RUN: &str = "Carry_Run_Loop";

/// A two-handed great weapon (#1073, asset-tooling #823): its item, the full-body run that
/// carries it and its running strike. Each strike starts and ends on its run's first frame and
/// records its contact, which [`super::Fit`] meets with the zone's.
#[derive(Clone, Copy, Debug, Eq, PartialEq, strum::VariantArray)]
pub enum GreatWeapon {
    Maul,
    GreatHammer,
    GreatAxe,
    Greatsword,
}

impl GreatWeapon {
    /// The `humanoid-equipment` item.
    #[must_use]
    pub const fn item(self) -> &'static str {
        match self {
            Self::Maul => "equipment.warhammer.maul",
            Self::GreatHammer => "equipment.hammer.great",
            Self::GreatAxe => "equipment.axe.great",
            Self::Greatsword => "equipment.sword.great",
        }
    }

    /// The full-body run carrying it, in place of the light run.
    #[must_use]
    pub const fn run(self) -> &'static str {
        match self {
            Self::Greatsword => "TwoHand_Sword_Run_Loop",
            Self::Maul | Self::GreatHammer | Self::GreatAxe => "TwoHand_Run_Loop",
        }
    }

    /// Its running strike: the overhead slam of the maul and great hammer, the great axe's
    /// diagonal chop and the greatsword's flat sweep.
    #[must_use]
    pub const fn run_strike(self) -> &'static str {
        match self {
            Self::Maul | Self::GreatHammer => "TwoHand_Run_Strike",
            Self::GreatAxe => "TwoHand_Axe_Run_Strike",
            Self::Greatsword => "TwoHand_Sword_Run_Strike",
        }
    }
}

/// The guard a great weapon's running strike settles into (#1073, #900).
pub const GREAT_WEAPON_GUARD: &str = "TwoHand_Guard_Loop";
/// A great weapon's draw from and sheathe across the back (#1073), on the upper body; the
/// weapon changes sockets at each clip's `ready.swapSeconds`.
pub const GREAT_WEAPON_DRAW: &str = "TwoHand_Draw";
pub const GREAT_WEAPON_SHEATHE: &str = "TwoHand_Sheathe";
/// The stance a great weapon's holder stands in with it drawn (#1073), over the idle legs.
pub const GREAT_WEAPON_IDLE: &str = "TwoHand_Idle_Loop";

/// A movement-state strike's clips (#885), best first: the family's authored strike
/// (`<Family>_Run_Strike`, `<Family>_Jump_Strike` and the chain, asset-tooling #709 and #711),
/// then the clip already in the bundle that stands in for it.
#[must_use]
pub const fn melee_clips(family: WeaponFamily, variant: StrikeVariant) -> [&'static str; 2] {
    use StrikeVariant::{Finisher, First, Jumping, Running, Second};
    match (family, variant) {
        (WeaponFamily::Sword, First) => ["Sword_Regular_A", "Sword_Slash_Mid_R"],
        (WeaponFamily::Sword, Second) => ["Sword_Regular_B", "Sword_Slash_Mid_L"],
        (WeaponFamily::Sword, Finisher) => ["Sword_Regular_C", "Sword_Attack"],
        (WeaponFamily::Sword, Running) => ["Sword_Run_Strike", "Sword_Slash_Mid_R"],
        (WeaponFamily::Sword, Jumping) => ["Sword_Jump_Strike", "Sword_Slash_High_C"],
        (WeaponFamily::Mace, First) => ["Mace_Swing_A", "Mace_Swing_A"],
        (WeaponFamily::Mace, Second) => ["Mace_Swing_B", "Mace_Swing_B"],
        (WeaponFamily::Mace, Finisher) => ["Mace_Swing_C", "Mace_Swing_A"],
        (WeaponFamily::Mace, Running) => ["Mace_Run_Strike", "Mace_Swing_B"],
        (WeaponFamily::Mace, Jumping) => ["Mace_Jump_Strike", "Mace_Swing_A"],
        (WeaponFamily::Dagger, First) => ["Dagger_Stab_R", "Dagger_Stab_R"],
        (WeaponFamily::Dagger, Second) => ["Dagger_Slash_L", "Dagger_Slash_L"],
        (WeaponFamily::Dagger, Finisher) => ["Dagger_Sinister", "Dagger_Sinister"],
        (WeaponFamily::Dagger, Running) => ["Dagger_Run_Strike", "Dagger_Stab_R"],
        (WeaponFamily::Dagger, Jumping) => ["Dagger_Jump_Strike", "Dagger_Sinister"],
        (WeaponFamily::Staff, First) => ["Staff_Strike_A", "Staff_Strike_A"],
        (WeaponFamily::Staff, Second) => ["Staff_Strike_B", "Staff_Strike_B"],
        (WeaponFamily::Staff, Finisher) => ["Staff_Strike_C", "Staff_Strike_B"],
        (WeaponFamily::Staff, Running) => ["Staff_Run_Strike", "Staff_Strike_A"],
        (WeaponFamily::Staff, Jumping) => ["Staff_Jump_Strike", "Staff_Strike_B"],
        (WeaponFamily::Spear, First | Second) => ["Spear_Thrust_A", "Staff_Strike_A"],
        (WeaponFamily::Spear, Finisher) => ["Spear_Thrust_B", "Staff_Strike_B"],
        (WeaponFamily::Spear, Running) => ["Spear_Run_Strike", "Staff_Strike_A"],
        (WeaponFamily::Spear, Jumping) => ["Spear_Jump_Strike", "Staff_Strike_B"],
        (WeaponFamily::Unarmed, First) => ["Unarmed_Jab", "Dagger_Stab_R"],
        (WeaponFamily::Unarmed, Second) => ["Unarmed_Cross", "Dagger_Slash_L"],
        (WeaponFamily::Unarmed, Finisher) => ["Unarmed_Hook", "Dagger_Gouge"],
        (WeaponFamily::Unarmed, Running) => ["Unarmed_Run_Strike", "Dagger_Stab_R"],
        (WeaponFamily::Unarmed, Jumping) => ["Unarmed_Jump_Strike", "Dagger_Gouge"],
        (WeaponFamily::Bow, First | Second) => ["Bow_Bash_A", "Sword_Slash_Mid_R"],
        (WeaponFamily::Bow, Finisher) => ["Bow_Bash_B", "Sword_Slash_High_C"],
        (WeaponFamily::Bow, Running) => ["Bow_Run_Strike", "Sword_Slash_Mid_R"],
        (WeaponFamily::Bow, Jumping) => ["Bow_Jump_Strike", "Sword_Slash_High_C"],
        (WeaponFamily::TwoHanded, First) => ["TwoHand_Swing_A", "Mace_Swing_A"],
        (WeaponFamily::TwoHanded, Second) => ["TwoHand_Swing_B", "Mace_Swing_B"],
        (WeaponFamily::TwoHanded, Finisher) => ["TwoHand_Swing_C", "Mace_Swing_C"],
        (WeaponFamily::TwoHanded, Running) => ["TwoHand_Run_Strike", "Mace_Run_Strike"],
        (WeaponFamily::TwoHanded, Jumping) => ["TwoHand_Jump_Strike", "Mace_Jump_Strike"],
    }
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

/// The social and interaction clips (#619, asset-tooling #421): all 30 fps and by name.
#[derive(Clone, Copy, Debug)]
pub struct SocialClips {
    pub wave: &'static str,
    pub point: &'static str,
    pub bow: &'static str,
    pub cheer: &'static str,
    pub laugh: &'static str,
    pub door_push: &'static str,
    pub door_pull: &'static str,
    pub door_knock: &'static str,
    pub loot_enter: &'static str,
    pub loot_loop: &'static str,
    pub loot_exit: &'static str,
    pub talk_idle: &'static str,
    pub talk_explain: &'static str,
    /// A line that points at an objective: the NPC's left faces it (#619).
    pub talk_direct: &'static str,
    pub talk_shrug: &'static str,
    pub trade: &'static str,
    pub sit_chair_enter: &'static str,
    pub sit_chair_loop: &'static str,
    pub sit_chair_exit: &'static str,
    pub sit_ground: &'static str,
    pub sleep_enter: &'static str,
    pub sleep_loop: &'static str,
    /// A knocked-awake resident getting up, over the zone's 1.5 s rise (#619).
    pub sleep_wake: &'static str,
}

impl SocialClips {
    /// The emote's clip and whether it takes the whole body (the bow; the others are
    /// upper-body and play over any gait).
    #[must_use]
    pub const fn emote(self, emote: mmorpg_core::Emote) -> (&'static str, bool) {
        match emote {
            mmorpg_core::Emote::Wave => (self.wave, false),
            mmorpg_core::Emote::Point => (self.point, false),
            mmorpg_core::Emote::Bow => (self.bow, true),
            mmorpg_core::Emote::Cheer => (self.cheer, false),
            mmorpg_core::Emote::Laugh => (self.laugh, false),
        }
    }

    /// A posture's chain: `enter` if it has one, the loop, and `exit` if it has one.
    #[must_use]
    pub const fn chain(
        self,
        posture: Posture,
    ) -> (Option<&'static str>, &'static str, Option<&'static str>) {
        match posture {
            Posture::Loot => (Some(self.loot_enter), self.loot_loop, Some(self.loot_exit)),
            Posture::SitChair => (
                Some(self.sit_chair_enter),
                self.sit_chair_loop,
                Some(self.sit_chair_exit),
            ),
            Posture::SitGround => (None, self.sit_ground, None),
            Posture::Sleep => (Some(self.sleep_enter), self.sleep_loop, None),
            // The rise plays once and holds its last frame until the zone moves the resident.
            Posture::Waking => (Some(self.sleep_wake), self.sleep_wake, None),
        }
    }

    /// Whether `clip` is a bed or chair clip, for which the drawn root moves off the body's spot
    /// onto the furniture.
    #[must_use]
    pub fn on_furniture(self, clip: &str) -> bool {
        clip == self.sleep_enter
            || clip == self.sleep_loop
            || clip == self.sleep_wake
            || clip == self.sit_chair_enter
            || clip == self.sit_chair_loop
            || clip == self.sit_chair_exit
    }
}

/// A whole-body pose a unit holds while it is still (#619): kneeling at a corpse, a seat or a
/// bed. It enters, loops and leaves by its clips ([`SocialClips::chain`]). Movement, damage, a
/// strike or death breaks it at once, and it stays broken until the cue goes away, so a looping
/// sit or sleep never pins a unit that walks.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Posture {
    Loot,
    SitChair,
    SitGround,
    Sleep,
    Waking,
}

impl Posture {
    /// How long into the exit moving input may cut it (the clip's `standSeconds`).
    pub(super) const fn stand_seconds(self) -> f32 {
        match self {
            Self::Loot => 0.075,
            Self::SitChair => 0.12,
            Self::SitGround | Self::Sleep | Self::Waking => 0.,
        }
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
    /// The forward run authored at the player's run speed (asset-tooling #746); without it the
    /// model's jog ([`Clips::run`]) runs.
    pub run: &'static str,
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
    /// The light hits left and right and the heavy hit from the front (asset-tooling #711),
    /// played for an ordinary and a critical hit when the bundle has them (#885).
    pub light: [&'static str; 2],
    pub heavy: &'static str,
    /// The upper body straining against a root and against ice (asset-tooling #767), looped
    /// while the zone holds the unit (#956), for a model that has them.
    pub rooted: Option<[&'static str; 2]>,
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
    pub(super) const fn region(self) -> Region {
        match self {
            Self::Chest | Self::Head => Region::Footwork,
            Self::Knockback | Self::Shove => Region::Whole,
            Self::Block | Self::GuardBreak => Region::Upper,
        }
    }

    /// An ordinary hit never cuts the unit's own strike short.
    pub(super) const fn interrupts(self) -> bool {
        !matches!(self, Self::Chest | Self::Head)
    }
}

/// Which body regions a one-shot drives.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Region {
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

impl Kind {
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Ranger => "Ranger",
            Self::Wolf => "Wolf",
            Self::Rat => "Rat",
            Self::Boar => "Boar",
            Self::Bear => "Bear",
            Self::Spider => "Spider",
            Self::Lurker => "Lurker",
            Self::Lynx => "Lynx",
            Self::Adder => "Adder",
            Self::Bat => "Bat",
            Self::Crayfish => "Crayfish",
            Self::Stag => "Stag",
        }
    }

    /// Its entry in the `greyhaven-wildlife` release (#889), for a beast.
    #[must_use]
    pub const fn wild_key(self) -> Option<&'static str> {
        match self {
            Self::Ranger => None,
            Self::Wolf => Some("greyhaven.wild.wolf"),
            Self::Rat => Some("greyhaven.wild.rat"),
            Self::Boar => Some("greyhaven.wild.boar"),
            Self::Bear => Some("greyhaven.wild.bear"),
            Self::Spider => Some("greyhaven.wild.spider"),
            Self::Lurker => Some("greyhaven.wild.marsh-lurker"),
            Self::Lynx => Some("greyhaven.wild.lynx"),
            Self::Adder => Some("greyhaven.wild.marsh-adder"),
            Self::Bat => Some("greyhaven.wild.cave-bat"),
            Self::Crayfish => Some("greyhaven.wild.river-crayfish"),
            Self::Stag => Some("greyhaven.wild.stag"),
        }
    }

    /// How far a dead one is drawn below its feet, in metres (#995): the cave bat's model is
    /// posed around its body centre, its rest pose's lowest point 0.153 m below it, and lies
    /// dead with that centre 0.062 m over the ground (the descriptor's `flight.groundHeight`).
    /// Every other model lies on its feet.
    #[must_use]
    pub const fn corpse_drop(self) -> f64 {
        match self {
            Self::Bat => 0.153 - 0.062,
            _ => 0.,
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
                deaths: Some(["Death_Back", "Death_Fwd", "Death_Spin"]),
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
                    run: "Run_Fwd_Loop",
                    walk_back: "Walk_Bwd_Loop",
                    walk_left: "Walk_Strafe_Left_Loop",
                    walk_right: "Walk_Strafe_Right_Loop",
                    run_back: "Jog_Bwd_Loop",
                    run_left: "Run_Strafe_Left_Loop",
                    run_right: "Run_Strafe_Right_Loop",
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
                    light: ["Hit_Light_L", "Hit_Light_R"],
                    heavy: "Hit_Heavy_Front",
                    rooted: Some(["Rooted_Struggle_Loop", "Rooted_Struggle_Frozen_Loop"]),
                }),
                sword: Some(SwordClips {
                    slashes: [
                        [
                            "Sword_Slash_Low_L",
                            "Sword_Slash_Low_C",
                            "Sword_Slash_Low_R",
                        ],
                        [
                            "Sword_Slash_Mid_L",
                            "Sword_Slash_Mid_C",
                            "Sword_Slash_Mid_R",
                        ],
                        [
                            "Sword_Slash_High_L",
                            "Sword_Slash_High_C",
                            "Sword_Slash_High_R",
                        ],
                    ],
                    heroic: "Sword_Heroic",
                    cleave: "Sword_Cleave",
                    charge_run: "Charge_Run_Loop",
                    charge_impact: "Charge_Impact",
                    guards: [
                        "Shield_Guard_Low_Loop",
                        "Shield_Guard_Mid_Loop",
                        "Shield_Guard_High_Loop",
                    ],
                    stun: "Stun_Loop",
                    respawn: "Respawn_Rise",
                    whirl: "Sword_Whirl",
                    smash: "Sword_Overhead_Smash",
                    sidesteps: ["Sword_Sidestep_L", "Sword_Sidestep_R"],
                    hop_back: "Sword_Hop_Back",
                    shield_guard: "Shield_Guard_Loop",
                    shield_turns: ["Shield_Guard_Turn_L", "Shield_Guard_Turn_R"],
                    shield_impact: "Shield_Block_Impact",
                    shield_stagger: "Guard_Broken_Stagger",
                }),
                dagger: Some(DaggerClips {
                    idle: "Dagger_Idle_Loop",
                    stab: "Dagger_Stab_R",
                    slash: "Dagger_Slash_L",
                    sinister: "Dagger_Sinister",
                    poison: "Dagger_Poison",
                    gouge: "Dagger_Gouge",
                    flourish: "Flourish_Haste",
                    throw_knife: "Throw_Knife",
                    throw_lob: "Throw_Lob",
                    bandage: "Bandage_Self",
                }),
                bow: Some(BowClips {
                    upper: CastClips {
                        aim: Some("Bow_Aim_Upper_Loop"),
                        enter: "Bow_Draw_Upper",
                        hold: "Bow_Hold_Upper_Loop",
                        recovery: Some("Bow_Recovery_Upper"),
                        cancel: "Bow_Cancel_Upper",
                        footwork: false,
                    },
                    release: "Bow_Release_Upper",
                    rapid: "Bow_Rapid_Release",
                    power_draw: "Bow_Power_Draw",
                }),
                crossbow: Some(CrossbowClips {
                    idle: "Crossbow_Idle_Loop",
                    aim: "Crossbow_Aim_Loop",
                    fire: "Crossbow_Fire",
                    reload: "Crossbow_Reload",
                }),
                caster: Some(CasterClips {
                    charge: "Cast_Charge_Loop",
                    push: "Cast_Release_Push",
                    overhand: "Cast_Release_Overhand",
                    beam: "Cast_Channel_Beam_Loop",
                    drain: "Cast_Channel_Drain_Loop",
                    sky: "Cast_Channel_Sky_Loop",
                    nova: "Cast_Nova",
                    cone: "Cast_Cone",
                    curse: "Cast_Curse_Point",
                    ground: "Cast_Ground_Sweep",
                    ward: "Cast_Self_Ward",
                    ally: "Cast_Ally",
                    warcry: "Warcry",
                    summon_raise: "Cast_Summon_Raise",
                    summon_call: "Cast_Summon_Call",
                    staff: ["Staff_Strike_A", "Staff_Strike_B"],
                    mace: ["Mace_Swing_A", "Mace_Swing_B"],
                    wall: "Cast_Raise_Wall",
                    interrupted: "Cast_Interrupted",
                }),
                social: Some(SocialClips {
                    wave: "Emote_Wave",
                    point: "Emote_Point",
                    bow: "Emote_Bow",
                    cheer: "Emote_Cheer",
                    laugh: "Emote_Laugh",
                    door_push: "Door_Push",
                    door_pull: "Door_Pull",
                    door_knock: "Door_Knock",
                    loot_enter: "Loot_Kneel_Enter",
                    loot_loop: "Loot_Kneel_Loop",
                    loot_exit: "Loot_Kneel_Exit",
                    talk_idle: "Talk_Idle_Loop",
                    talk_explain: "Talk_Gesture_A",
                    talk_direct: "Talk_Gesture_B",
                    talk_shrug: "Talk_Gesture_C",
                    trade: "Trade_Handover",
                    sit_chair_enter: "Sit_Chair_Enter",
                    sit_chair_loop: "Sit_Chair_Loop",
                    sit_chair_exit: "Sit_Chair_Exit",
                    sit_ground: "Sit_Ground_Loop",
                    sleep_enter: "Sleep_Bed_Enter",
                    sleep_loop: "Sleep_Bed_Loop",
                    sleep_wake: "Sleep_Bed_Wake",
                }),
                rest: None,
                bites: None,
            },
            // The wildlife release's beasts (#889) share one clip set.
            Self::Wolf | Self::Rat | Self::Boar | Self::Bear | Self::Spider | Self::Lurker => {
                WILD_CLIPS
            }
            Self::Lynx => LYNX_CLIPS,
            Self::Adder => ADDER_CLIPS,
            Self::Bat => BAT_CLIPS,
            // The river crayfish (#996) has the beasts' clip set too; its `Attack_A` claw raise
            // and pinch is its one attack.
            Self::Crayfish => WILD_CLIPS,
            Self::Stag => STAG_CLIPS,
        }
    }

    /// The authored strike an accepted attack or ability of a `class` user plays: the Warden's
    /// Shield Bash its shield-arm clip, Heroic Strike the slash toward its `aim` or, undirected,
    /// the heroic strike, Cleave its sweep, the Redbrand whirl and smash their own (#582); a bow
    /// class's ability its bow release, a staff or spell class's ability its spell release; a
    /// basic swing (no ability) of any class, and everything else, the slash toward the swing's
    /// `aim`, or its attack (#884). A model without them plays its attack.
    #[must_use]
    pub fn strike(
        self,
        class: Option<PlayerClass>,
        ability: Option<AbilityId>,
        aim: Option<StrikeAim>,
    ) -> &'static str {
        use mmorpg_core::ability::ids;
        let clips = self.clips();
        // A Druid in wolf form (#957) savages with the wolf's second bite.
        if ability == Some(ids::SAVAGE_BITE)
            && let Some(bites) = clips.bites
        {
            return bites.attacks[1];
        }
        let Some(strikes) = clips.strikes else {
            return clips.attack;
        };
        if ability == Some(ids::SHIELD_BASH) {
            return strikes.shield_bash;
        }
        if let Some(sword) = clips.sword
            && let Some(ability) = ability
        {
            match (ability, aim) {
                (ids::HEROIC_STRIKE, Some(aim)) => return sword.slash(aim),
                (ids::HEROIC_STRIKE, None) => return sword.heroic,
                (ids::CLEAVE, _) => return sword.cleave,
                (ids::REDBRAND_WHIRL | ids::REAVERS_SWEEP | ids::LAST_COMMAND, _) => {
                    return sword.whirl;
                }
                (ids::OVERHEAD_SMASH, _) => return sword.smash,
                _ => {}
            }
        }
        match class.filter(|_| ability.is_some()) {
            Some(PlayerClass::Ranger) => strikes.bow,
            Some(
                PlayerClass::Arcanist
                | PlayerClass::Necromancer
                | PlayerClass::Cleric
                | PlayerClass::Druid,
            ) => strikes.spell,
            Some(PlayerClass::Warden | PlayerClass::Rogue) | None => clips
                .sword
                .zip(aim)
                .map_or(clips.attack, |(sword, aim)| sword.slash(aim)),
        }
    }

    /// The clip a movement-state strike (#885) of a `family` weapon plays and the body it
    /// drives, for a model with authored strikes: the family's own strike (asset-tooling #709,
    /// #711) when the model `has` it, otherwise the clip that stands in for it until that
    /// release is fetched. Authored running and jumping strikes move the whole body, the
    /// full-body standing ones ([`FULL_BODY_STRIKES`], #900) the whole body while the unit stands;
    /// other standing ones, and every stand-in for a running or jumping strike, only the upper
    /// body, over the legs' run or jump.
    #[must_use]
    pub fn melee_strike(
        self,
        family: WeaponFamily,
        variant: StrikeVariant,
        has: impl Fn(&str) -> bool,
    ) -> Option<(&'static str, Region)> {
        self.clips().strikes?;
        let [authored, stand_in] = melee_clips(family, variant);
        let moving = matches!(variant, StrikeVariant::Running | StrikeVariant::Jumping);
        if has(authored) {
            let region = if moving {
                Region::Whole
            } else if FULL_BODY_STRIKES.contains(&authored) {
                Region::Footwork
            } else {
                Region::Upper
            };
            return Some((authored, region));
        }
        Some((
            stand_in,
            if moving {
                Region::Upper
            } else {
                self.strike_region(stand_in)
            },
        ))
    }

    /// The authored dagger clip an accepted attack or ability plays (#603), and the throwable a
    /// throw draws: a dagger holder's auto-attack stabs, and with a dagger in each hand
    /// alternates the stab (even `swing`s, counting from 0) with the left dagger's slash; the
    /// Rogue's abilities, and the Ranger's Rapid Fire, play their own. `None` for anything else
    /// or a model without them; [`Self::strike`] then decides.
    #[must_use]
    pub fn dagger_strike(
        self,
        loadout: Option<Loadout>,
        ability: Option<AbilityId>,
        swing: u32,
    ) -> Option<(&'static str, Option<Throwable>)> {
        use mmorpg_core::ability::ids;
        let dagger = self.clips().dagger?;
        let clip = match ability {
            None if loadout.is_some_and(Loadout::daggers) => {
                if loadout == Some(Loadout::TwinDaggers) && swing % 2 == 1 {
                    dagger.slash
                } else {
                    dagger.stab
                }
            }
            Some(ids::SINISTER_STRIKE) => dagger.sinister,
            Some(ids::POISONED_BLADE) => dagger.poison,
            Some(ids::GOUGE) => dagger.gouge,
            Some(ids::ADRENALINE_RUSH | ids::RAPID_FIRE) => dagger.flourish,
            Some(ids::THROWING_KNIFE) => return Some((dagger.throw_knife, Some(Throwable::Knife))),
            Some(ids::VENOM_FLASK) => return Some((dagger.throw_lob, Some(Throwable::Flask))),
            Some(ids::BLASTING_BOMB) => return Some((dagger.throw_lob, Some(Throwable::Bomb))),
            _ => return None,
        };
        Some((clip, None))
    }

    /// The authored ranged clip an accepted shot plays instead of the bow release (#607): a
    /// crossbowman's every shot fires the crossbow; under Rapid Fire's haste every bow shot is
    /// the rapid release; Aimed Shot and Piercing Shot loose with the upper-body release after
    /// their power draw. `None` for any other shot or a model without them; the bow release
    /// then plays.
    #[must_use]
    pub fn ranged_strike(
        self,
        loadout: Option<Loadout>,
        ability: Option<AbilityId>,
        hasted: bool,
    ) -> Option<&'static str> {
        use mmorpg_core::ability::ids;
        let clips = self.clips();
        if loadout == Some(Loadout::Crossbow) {
            return clips.crossbow.map(|crossbow| crossbow.fire);
        }
        let bow = clips.bow?;
        if hasted {
            return Some(bow.rapid);
        }
        matches!(ability, Some(ids::AIMED_SHOT | ids::PIERCING_SHOT)).then_some(bow.release)
    }

    /// The draw the local player's wind-up of `ability` shows instead of the aim (#607): Aimed
    /// Shot's and Piercing Shot's power draw, held at its end until the key's release sends the
    /// shot and the zone answers.
    #[must_use]
    pub fn power_draw(self, ability: AbilityId) -> Option<&'static str> {
        use mmorpg_core::ability::ids;
        self.clips()
            .bow
            .filter(|_| matches!(ability, ids::AIMED_SHOT | ids::PIERCING_SHOT))
            .map(|bow| bow.power_draw)
    }

    /// How a caster shows its cast or channel of `ability` (#609): the bolts and blasts hold the
    /// charge (Call Companion the charge and then its whistle), the beams, the drain and
    /// Blizzard their channel loops, Raise Skeleton its kneel from the cast's start. `None` for
    /// any other ability or a model without them.
    #[must_use]
    pub fn cast_style(self, ability: AbilityId) -> Option<CastStyle> {
        use mmorpg_core::ability::ids;
        let caster = self.clips().caster?;
        Some(match ability {
            ids::FIREBOLT
            | ids::FIRE_BLAST
            | ids::CHAIN_LIGHTNING
            | ids::SHADOW_BOLT
            | ids::SMITE
            | ids::WRATH
            | ids::MUCK_BOLT => CastStyle::Charge(caster.charge),
            ids::LIGHTNING_BEAM | ids::HOLY_BEAM => CastStyle::Channel(caster.beam),
            ids::DRAIN_LIFE => CastStyle::Channel(caster.drain),
            ids::BLIZZARD => CastStyle::Channel(caster.sky),
            ids::RAISE_SKELETON => CastStyle::Whole(caster.summon_raise),
            ids::CALL_COMPANION => CastStyle::Finish(caster.charge, caster.summon_call),
            _ => return None,
        })
    }

    /// The caster clip an accepted attack or ability plays (#609): a staff holder's auto-attack
    /// alternates the jab (even `swing`s, counting from 0) and the sweep, a mace holder's the
    /// overhand and the backhand; a completed bolt pushes from the charge, a blast throws
    /// overhand; the instants play their style, Divine Shield the ward on oneself and the touch
    /// on an `ally`; Rallying Cry the war cry. `None` for anything else or a model without them;
    /// [`Self::strike`] then decides.
    #[must_use]
    pub fn caster_strike(
        self,
        loadout: Option<Loadout>,
        ability: Option<AbilityId>,
        ally: bool,
        swing: u32,
    ) -> Option<&'static str> {
        use mmorpg_core::ability::ids;
        let caster = self.clips().caster?;
        let alternate = |pair: [&'static str; 2]| pair[(swing % 2) as usize];
        Some(match ability {
            None => match loadout? {
                Loadout::BoneStaff | Loadout::CrystalStaffAndTome | Loadout::GnarledStaff => {
                    alternate(caster.staff)
                }
                Loadout::FlangedMace => alternate(caster.mace),
                _ => return None,
            },
            Some(ids::FIREBOLT | ids::SHADOW_BOLT | ids::SMITE | ids::WRATH | ids::MUCK_BOLT) => {
                caster.push
            }
            Some(ids::FIRE_BLAST | ids::CHAIN_LIGHTNING) => caster.overhand,
            Some(ids::FROST_NOVA | ids::STOMP) => caster.nova,
            Some(ids::FROST_BREATH) => caster.cone,
            Some(ids::CORRUPTION | ids::CORPSE_BURST) => caster.curse,
            Some(ids::RAISE_DEAD) => caster.summon_raise,
            Some(ids::ENTANGLING_ROOTS) => caster.ground,
            Some(ids::DIVINE_SHIELD) if ally => caster.ally,
            Some(ids::ARCANE_BARRIER | ids::BONE_SHIELD | ids::BARKSKIN | ids::DIVINE_SHIELD) => {
                caster.ward
            }
            Some(ids::RENEW | ids::REJUVENATION) => caster.ally,
            Some(ids::RALLYING_CRY) => caster.warcry,
            _ => return None,
        })
    }

    /// Which body a strike of `clip` drives (#582): an authored sword clip its own region, a
    /// dagger, upper-body bow or crossbow clip (#603, #607) the upper body, a caster clip its
    /// own (#609), every other strike the whole body standing and the upper body on the move.
    #[must_use]
    pub fn strike_region(self, clip: &str) -> Region {
        let clips = self.clips();
        if let Some(region) = clips.caster.and_then(|caster| caster.region(clip)) {
            return region;
        }
        if clips.dagger.is_some_and(|dagger| dagger.contains(clip))
            || clips.bow.is_some_and(|bow| bow.contains(clip))
            || clips
                .crossbow
                .is_some_and(|crossbow| crossbow.contains(clip))
        {
            return Region::Upper;
        }
        clips
            .sword
            .and_then(|sword| sword.region(clip))
            .unwrap_or(Region::Footwork)
    }

    /// The cast cycle a `class` user carrying `loadout` shows while casting or channelling
    /// (#538): a crossbowman holds its aim (#607), a bow class draws and holds the bow,
    /// everyone else holds a spell.
    #[must_use]
    pub fn cast(self, class: Option<PlayerClass>, loadout: Option<Loadout>) -> Option<CastClips> {
        let clips = self.clips();
        if let Some(crossbow) = clips
            .crossbow
            .filter(|_| loadout == Some(Loadout::Crossbow))
        {
            return Some(crossbow.cycle());
        }
        let strikes = clips.strikes?;
        Some(if class == Some(PlayerClass::Ranger) {
            strikes.bow_cycle
        } else {
            strikes.spell_cycle
        })
    }

    /// The cast cycle whose release `strike` is, for its wind-up and recovery (#538): the
    /// upper-body release's the upper-body cycle, the crossbow's shot the reload (#607).
    #[must_use]
    pub fn cycle_of(self, strike: &str) -> Option<CastClips> {
        let clips = self.clips();
        let strikes = clips.strikes?;
        if strike == strikes.bow {
            Some(strikes.bow_cycle)
        } else if strike == strikes.spell {
            Some(strikes.spell_cycle)
        } else if let Some(bow) = clips.bow.filter(|bow| bow.release == strike) {
            Some(bow.upper)
        } else {
            clips
                .crossbow
                .filter(|crossbow| crossbow.fire == strike)
                .map(CrossbowClips::cycle)
        }
    }

    /// The bow shot `animation` shows (#556): its upper-body target is the bow release, a clip
    /// of the bow cycle or one of the upper-body bow clips (#607), whose `bow.drawMeters` the
    /// nock follows alike.
    #[must_use]
    pub fn bow_shot(self, animation: &Animation) -> Option<BowShot> {
        let clips = self.clips();
        let strikes = clips.strikes?;
        let (clip, time) = animation.upper;
        (clip == strikes.bow
            || strikes.bow_cycle.contains(clip)
            || clips.bow.is_some_and(|bow| bow.contains(clip)))
        .then_some(BowShot { clip, time })
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
            // A Druid in wolf form (#957) is drawn as the wildlife release's wolf.
            EntityRef::Player(_) if record.flags.shapeshifted => Some(Self::Wolf),
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
            CreatureFamily::Boar => Some(Self::Boar),
            CreatureFamily::Bear => Some(Self::Bear),
            CreatureFamily::Spider => Some(Self::Spider),
            CreatureFamily::Lurker => Some(Self::Lurker),
            CreatureFamily::Lynx => Some(Self::Lynx),
            CreatureFamily::Adder => Some(Self::Adder),
            CreatureFamily::Bat => Some(Self::Bat),
            CreatureFamily::Crayfish => Some(Self::Crayfish),
            CreatureFamily::Stag => Some(Self::Stag),
            CreatureFamily::Marauder
            | CreatureFamily::Mirefin
            | CreatureFamily::Redbrand
            | CreatureFamily::Ogre
            | CreatureFamily::Prop => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mmorpg_core::{CreatureId, NpcId, greyhaven_vale};
    use strum::VariantArray;

    use crate::units::swim::Attitude;

    #[test]
    fn movement_strikes_play_the_family_clip_or_its_stand_in() {
        // Before the asset-tooling #709/#711 release, the existing clips stand in (#885).
        let without = |clip: &str| !clip.contains("_Run_") && !clip.contains("_Jump_");
        assert_eq!(
            Kind::Ranger.melee_strike(WeaponFamily::Sword, StrikeVariant::Running, without),
            Some(("Sword_Slash_Mid_R", Region::Upper))
        );
        assert_eq!(
            Kind::Ranger.melee_strike(WeaponFamily::Sword, StrikeVariant::Running, |_| true),
            Some(("Sword_Run_Strike", Region::Whole))
        );
        assert_eq!(
            Kind::Ranger.melee_strike(WeaponFamily::Spear, StrikeVariant::Finisher, |_| false),
            Some((
                "Staff_Strike_B",
                Kind::Ranger.strike_region("Staff_Strike_B")
            ))
        );
        assert_eq!(
            Kind::Ranger.melee_strike(WeaponFamily::Mace, StrikeVariant::Finisher, |_| true),
            Some(("Mace_Swing_C", Region::Footwork))
        );
        assert_eq!(
            Kind::Wolf.melee_strike(WeaponFamily::Unarmed, StrikeVariant::First, |_| true),
            None
        );
        // The chain reads as three different blows for every family.
        for family in WeaponFamily::ALL {
            let [first, _] = melee_clips(family, StrikeVariant::First);
            let [finisher, _] = melee_clips(family, StrikeVariant::Finisher);
            assert_ne!(first, finisher, "{family:?}");
        }
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
        // A Druid in wolf form (#957) is the wolf, with its run and bites.
        let mut shifted = content_record();
        shifted.flags.shapeshifted = true;
        assert_eq!(
            Kind::of(EntityRef::Player(1), &shifted, &content),
            Some(Kind::Wolf)
        );
        assert_eq!(creature(CreatureFamily::Wolf), Some(Kind::Wolf));
        assert_eq!(creature(CreatureFamily::Vermin), Some(Kind::Rat));
        assert_eq!(creature(CreatureFamily::Boar), Some(Kind::Boar));
        assert_eq!(creature(CreatureFamily::Bear), Some(Kind::Bear));
        assert_eq!(creature(CreatureFamily::Spider), Some(Kind::Spider));
        assert_eq!(creature(CreatureFamily::Lurker), Some(Kind::Lurker));
        assert_eq!(creature(CreatureFamily::Lynx), Some(Kind::Lynx));
        assert_eq!(creature(CreatureFamily::Adder), Some(Kind::Adder));
        assert_eq!(creature(CreatureFamily::Bat), Some(Kind::Bat));
        assert_eq!(creature(CreatureFamily::Crayfish), Some(Kind::Crayfish));
        assert_eq!(creature(CreatureFamily::Stag), Some(Kind::Stag));
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

    #[test]
    fn the_bow_cycle_holds_the_bow_and_its_draw_samples_between_frames() {
        let shows = |clip: &'static str| Animation {
            layers: vec![Layer::single(clip, 0.2)],
            lower: (clip, 0.2),
            upper: (clip, 0.2),
            attitude: Attitude::default(),
            carry: Carry::default(),
            channel: None,
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

    /// Each beast rests and walks in the clips the fetched wildlife descriptor names
    /// (`restClip`, `moveClip`), and every attack clip its table plays is in the descriptor.
    #[test]
    fn the_beasts_clip_tables_follow_the_fetched_wildlife_descriptor() {
        let bundle = format!(
            "{}/../../web/public/scenery/greyhaven.wildlife",
            env!("CARGO_MANIFEST_DIR")
        );
        let Ok(manifest) = std::fs::read_to_string(format!("{bundle}/manifest.json")) else {
            eprintln!("the wildlife release is not fetched; skipping");
            return;
        };
        let manifest: serde_json::Value = serde_json::from_str(&manifest).unwrap();
        let path = manifest["assets"]
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|asset| asset["attachments"].as_array().unwrap())
            .find(|attachment| attachment["name"] == "wildlife-descriptor")
            .and_then(|attachment| attachment["path"].as_str())
            .unwrap();
        let descriptor: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(format!("{bundle}/{path}")).unwrap())
                .unwrap();
        for &kind in Kind::VARIANTS {
            let Some(key) = kind.wild_key() else {
                continue;
            };
            let entry = descriptor["assets"]
                .as_array()
                .unwrap()
                .iter()
                .find(|entry| entry["key"] == key)
                .unwrap_or_else(|| panic!("the descriptor lists {key}"));
            let clips = kind.clips();
            assert_eq!(entry["restClip"], clips.idle, "{key} rests");
            assert_eq!(entry["moveClip"], clips.walk, "{key} walks");
            let bites = clips.bites.unwrap();
            for clip in bites.attacks.into_iter().chain(
                bites
                    .follow
                    .into_iter()
                    .flat_map(|(then, last)| std::iter::once(then).chain(last)),
            ) {
                assert!(entry["clips"][clip].is_object(), "{key} has {clip}");
            }
        }
    }

    #[test]
    fn strikes_use_the_class_and_ability_clips_the_bundle_has() {
        use mmorpg_core::ability::ids;
        let warden = Some(PlayerClass::Warden);
        let aim = |side, height| Some(StrikeAim::new(side, height));
        assert_eq!(
            Kind::Ranger.strike(warden, Some(ids::HEROIC_STRIKE), None),
            "Sword_Heroic"
        );
        // A directed Heroic Strike and an ordinary swing slash toward their aim (#582).
        assert_eq!(
            Kind::Ranger.strike(
                warden,
                Some(ids::HEROIC_STRIKE),
                aim(mmorpg_core::StrikeSide::Left, StrikeHeight::High)
            ),
            "Sword_Slash_High_L"
        );
        assert_eq!(
            Kind::Ranger.strike(
                warden,
                None,
                aim(mmorpg_core::StrikeSide::Right, StrikeHeight::Low)
            ),
            "Sword_Slash_Low_R"
        );
        assert_eq!(
            Kind::Ranger.strike(warden, Some(ids::CLEAVE), None),
            "Sword_Cleave"
        );
        assert_eq!(
            Kind::Ranger.strike(None, Some(ids::REDBRAND_WHIRL), None),
            "Sword_Whirl"
        );
        assert_eq!(
            Kind::Ranger.strike(None, Some(ids::OVERHEAD_SMASH), None),
            "Sword_Overhead_Smash"
        );
        // A bow class's basic swing is a melee slash like everyone else's (#884); its shots keep
        // the release.
        assert_eq!(
            Kind::Ranger.strike(
                Some(PlayerClass::Ranger),
                None,
                aim(mmorpg_core::StrikeSide::Centre, StrikeHeight::Middle)
            ),
            "Sword_Slash_Mid_C"
        );
        assert_eq!(
            Kind::Ranger.strike(Some(PlayerClass::Ranger), Some(ids::SERPENT_STING), None),
            "Bow_Release"
        );
        assert_eq!(
            Kind::Ranger.strike_region("Sword_Slash_Mid_C"),
            Region::Upper
        );
        assert_eq!(Kind::Ranger.strike_region("Sword_Whirl"), Region::Whole);
        assert_eq!(Kind::Ranger.strike_region("Sword_Attack"), Region::Footwork);
        assert_eq!(
            Kind::Ranger.strike(Some(PlayerClass::Warden), Some(ids::SHIELD_BASH), None),
            "Shield_OneShot"
        );
        assert_eq!(
            Kind::Ranger.strike(Some(PlayerClass::Ranger), None, None),
            "Sword_Attack"
        );
        assert_eq!(
            Kind::Ranger.strike(Some(PlayerClass::Arcanist), None, None),
            "Sword_Attack"
        );
        assert_eq!(
            Kind::Ranger.strike(Some(PlayerClass::Arcanist), Some(ids::FIREBOLT), None),
            "Spell_Simple_Shoot"
        );
        assert_eq!(Kind::Ranger.strike(None, None, None), "Sword_Attack");
        assert_eq!(
            Kind::Wolf.strike(None, Some(ids::SHIELD_BASH), None),
            "Attack_A"
        );
    }

    #[test]
    fn dagger_holders_alternate_hands_and_play_the_rogue_ability_clips() {
        use mmorpg_core::ability::ids;
        let twin = Some(Loadout::TwinDaggers);
        let strike = |loadout, ability, swing| {
            Kind::Ranger
                .dagger_strike(loadout, ability, swing)
                .map(|(clip, _)| clip)
        };
        assert_eq!(Loadout::of_class(PlayerClass::Rogue), ROGUE_LOADOUT);
        assert_eq!(strike(twin, None, 0), Some("Dagger_Stab_R"));
        assert_eq!(strike(twin, None, 1), Some("Dagger_Slash_L"));
        assert_eq!(strike(twin, None, 2), Some("Dagger_Stab_R"));
        // One dagger only stabs; a sword holder keeps its slashes.
        assert_eq!(
            strike(Some(Loadout::Dagger), None, 1),
            Some("Dagger_Stab_R")
        );
        assert_eq!(strike(Some(Loadout::Sword), None, 1), None);
        for (ability, clip) in [
            (ids::SINISTER_STRIKE, "Dagger_Sinister"),
            (ids::POISONED_BLADE, "Dagger_Poison"),
            (ids::GOUGE, "Dagger_Gouge"),
            (ids::ADRENALINE_RUSH, "Flourish_Haste"),
            (ids::THROWING_KNIFE, "Throw_Knife"),
            (ids::VENOM_FLASK, "Throw_Lob"),
            (ids::BLASTING_BOMB, "Throw_Lob"),
        ] {
            assert_eq!(strike(twin, Some(ability), 0), Some(clip));
            assert_eq!(Kind::Ranger.strike_region(clip), Region::Upper);
        }
        // The Ranger's Rapid Fire flourishes empty-handed.
        assert_eq!(
            strike(Some(Loadout::BowAndQuiver), Some(ids::RAPID_FIRE), 0),
            Some("Flourish_Haste")
        );
        assert_eq!(
            Kind::Ranger.dagger_strike(twin, Some(ids::BLASTING_BOMB), 0),
            Some(("Throw_Lob", Some(Throwable::Bomb)))
        );
        assert_eq!(Kind::Wolf.dagger_strike(twin, None, 0), None);
    }

    #[test]
    fn rapid_fire_power_shots_and_crossbows_pick_their_ranged_clips() {
        use mmorpg_core::ability::ids;
        let ranger = Kind::Ranger;
        let bow = Some(Loadout::BowAndQuiver);
        assert_eq!(ranger.ranged_strike(bow, None, false), None);
        assert_eq!(
            ranger.ranged_strike(bow, Some(ids::SERPENT_STING), false),
            None
        );
        // Under Rapid Fire every shot is the rapid release, power shots too.
        assert_eq!(
            ranger.ranged_strike(bow, None, true),
            Some("Bow_Rapid_Release")
        );
        assert_eq!(
            ranger.ranged_strike(bow, Some(ids::AIMED_SHOT), true),
            Some("Bow_Rapid_Release")
        );
        for power in [ids::AIMED_SHOT, ids::PIERCING_SHOT] {
            assert_eq!(
                ranger.ranged_strike(bow, Some(power), false),
                Some("Bow_Release_Upper")
            );
            assert_eq!(ranger.power_draw(power), Some("Bow_Power_Draw"));
        }
        assert_eq!(ranger.power_draw(ids::RICOCHET_SHOT), None);
        assert_eq!(
            ranger
                .cycle_of("Bow_Release_Upper")
                .and_then(|cycle| cycle.recovery),
            Some("Bow_Recovery_Upper")
        );
        assert!(ranger.cycle_of("Bow_Rapid_Release").is_none());
        // A crossbowman fires and reloads, and holds its aim through Crossbow Bolt's cast.
        let crossbow = Some(Loadout::Crossbow);
        assert_eq!(
            Resident::RedbrandCrossbowman.loadout(),
            Some(Loadout::Crossbow)
        );
        assert_eq!(
            ranger.ranged_strike(crossbow, Some(ids::CROSSBOW_BOLT), false),
            Some("Crossbow_Fire")
        );
        assert_eq!(
            ranger
                .cycle_of("Crossbow_Fire")
                .and_then(|cycle| cycle.recovery),
            Some("Crossbow_Reload")
        );
        let cast = ranger
            .cast(Some(PlayerClass::Ranger), crossbow)
            .expect("a crossbow cast");
        assert_eq!(cast.hold, "Crossbow_Aim_Loop");
        for clip in ["Crossbow_Fire", "Bow_Rapid_Release", "Bow_Release_Upper"] {
            assert_eq!(ranger.strike_region(clip), Region::Upper, "{clip}");
        }
        assert_eq!(
            Loadout::Crossbow.hands().get(Hand::Left),
            Some(Held::Crossbow)
        );
        assert_eq!(Held::Crossbow.grip(Hand::Right), "Grip_Crossbow_R");
    }
}
