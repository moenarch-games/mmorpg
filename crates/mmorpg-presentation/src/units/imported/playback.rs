//! The per-unit clip state machine: which clip each body region plays, its clocks and the gait
//! and traversal speeds it keeps pace with.

use mmorpg_core::{Ledge, StrikeHeight, TraversalView, WaterMode, WeaponFamily};

use super::*;

use std::f64::consts::FRAC_PI_2;

use crate::units::chores::Chore;
use crate::units::swim::Attitude;
use crate::units::traversal::Motions;

/// Climbing travel along the face below this speed in metres per second holds on still.
pub const CLIMB_MOVING_SPEED: f64 = 0.15;
/// The speeds in metres per second at which a climb loop plays at its authored rate when the
/// bundle does not say (`climbSpeedMetersPerSecond`): the zone's climb up or down and sideways,
/// and the clips' ladder and rope step.
pub const CLIMB_CLIP_SPEED: f64 = 0.9;
pub const CLIMB_SIDE_CLIP_SPEED: f64 = 0.75;
pub const LADDER_CLIP_SPEED: f64 = 0.6;
/// Where the Ranger's ladder clips (clip root at the origin) hold the rungs (#621), as the
/// feet's height above the rung grid (modulo the 0.3 m pitch): the hold grips rungs at
/// root-relative 0.248 + 0.3k m, and the `contacts` of the up and down loops plant the first
/// hand and the feet on rungs when their loop starts at these heights.
pub const LADDER_IDLE_RUNG_OFFSET: f64 = 0.052;
pub const LADDER_UP_RUNG_OFFSET: f64 = 0.27;
pub const LADDER_DOWN_RUNG_OFFSET: f64 = 0.227;
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
/// backpedal, play at their authored rate on a model whose clips record no ground speed
/// ([`gait_rate`]).
pub const RUN_CLIP_GROUND: f64 = 5.;
/// Ground speed in metres per second above which forward travel sprints (#538): above the
/// player's 6.3 m/s run, so only a faster mover (a charge) sprints.
pub const SPRINT_CLIP_SPEED: f64 = 7.5;
/// Ground speed in metres per second at which the sprint plays at its authored rate: the run's,
/// scaled by the clips' `groundSpeedMetersPerSecond` (2.13 m/s sprint, 1.76 m/s jog).
pub const SPRINT_CLIP_GROUND: f64 = RUN_CLIP_GROUND * 2.1345 / 1.7637;
/// The slowest and fastest a gait with an authored ground speed plays ([`gait_rate`], #804). The
/// stride is authored (Atlas 6.6.12), so the rate is the legs' cadence: past 1.5 times it a
/// clip whose stride is too short for the speed (the Ranger's 1.04 m/s walk under a 2.4 m/s
/// creature walk, its 1.76 m/s jog under a 3.9 m/s backpedal) slides its feet rather than
/// spinning its legs at double speed. The 6.3 m/s runs keep the player's run at 1.
pub const GAIT_RATES: [f64; 2] = [0.5, 1.5];
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
/// The shield guard's planted pivots (#953, asset-tooling #774) turn 45° each.
const SHIELD_TURN_CLIP_RADIANS: f64 = std::f64::consts::FRAC_PI_4;
/// The longest a reaction plays, in seconds: a long shove spin stops here and blends out.
pub const REACTION_SECONDS: f32 = 1.5;
/// A cast's held strike wind-up (#582) waits this long past the cast for the zone's strike.
pub const WIND_UP_SLACK_SECONDS: f32 = 0.5;
/// A charge (#582) rushes at most this long: the zone's longest charge and a little slack.
pub const CHARGE_SECONDS: f64 =
    mmorpg_core::CHARGE_MAX_TICKS as f64 / mmorpg_core::TICK_HZ as f64 + 0.1;
/// After this long, a charger slowed below [`CHARGE_ARRIVAL_SPEED`] metres per second arrived.
pub const CHARGE_ARRIVAL_SECONDS: f64 = 0.2;
pub const CHARGE_ARRIVAL_SPEED: f64 = 4.;
/// The speed in metres per second at which the charge loop plays at its authored rate.
pub const CHARGE_CLIP_SPEED: f64 = 9.;

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
    /// Where the daggers ride and what the right hand throws (#603).
    pub carry: Carry,
    /// Where the spell of the caster clip shown starts (#609), once the client has read the
    /// clip's `cast` marks: the origin for a bolt, a beam or a drain.
    pub channel: Option<Channel>,
}

/// What the projection shows of one unit this frame. Every cue is zone-accepted state or the
/// projected motion; the clip clocks never feed back into it.
#[derive(Clone, Copy, Debug, Default)]
pub struct Cues<'a> {
    /// The zone's projected climb, mantle or roll (#490, #500, #516).
    pub traversal: TraversalView,
    /// The model's `humanoid-clips` traversal data, once loaded (#500).
    pub motions: Option<&'a Motions>,
    /// On a ladder, how far the drawn feet stand above its rung grid, in `[0, 0.3)` metres
    /// (`crate::climbing::rung_offset`): the ladder loops' phase follows it (#621).
    pub rungs: Option<f64>,
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
    /// The lane the zone's guard protects now: a ready, grounded raised shield (viewer only).
    pub guard: Option<StrikeHeight>,
    /// Under a stun the zone applied (#582).
    pub stunned: bool,
    /// Held by a root the zone applied (#956): Entangling Roots' vines or Frost Nova's ice.
    pub rooted: Option<crate::units::effects::Condition>,
    /// The zone projects a dodge roll (#516).
    pub rolling: bool,
    /// The zone projects the unit in combat: a crossbowman standing in combat aims (#607).
    pub engaged: bool,
    /// Explicit player mode, independent of threat. `None` leaves NPC/effect carry untouched.
    pub weapon_drawn: Option<bool>,
    /// The pose the unit should hold while still (#619): the viewer's loot kneel, or a resident
    /// asleep at its bed.
    pub posture: Option<Posture>,
    /// The upper body talks: a window of the viewer's is open toward this NPC (#619).
    pub talking: bool,
    /// The drawn body's turn from its facing, in radians, while it points at its target, kneels
    /// at a corpse or lies down (#619); `None` keeps the facing.
    pub face: Option<f64>,
    /// What the unit carries: each held item's grip pose closes the fingers round it
    /// (asset-tooling #285), and a dagger holder stands in the dagger stance and sheathes its
    /// daggers to throw or bandage (#603).
    pub loadout: Option<Loadout>,
    /// The chore a resident does on its routine now (#904): its item rides its socket while the
    /// unit stands or walks, with the chore's clips when the figure has them.
    pub chore: Option<crate::units::chores::Chore>,
    /// A fighting skirmisher, the Redbrand Bandit (#963): stepping aside or back from its
    /// facing plays the sword footwork ([`SwordClips::sidesteps`]) instead of the strafe or
    /// backpedal.
    pub footwork: bool,
    /// A shield-bearer fighting, the Redbrand Brute (#953): it stands in its round-shield guard,
    /// pivots with the guard turns, takes a blocked blow on the shield and staggers when one
    /// gets past it.
    pub shield_guard: bool,
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

/// The walk's rate at `speed` metres per second, for a model whose clips record no ground speed.
#[allow(clippy::cast_possible_truncation)]
fn walk_rate(speed: f64) -> f32 {
    (speed / 1.8).clamp(0.6, 1.6) as f32
}

/// The rate at which the in-place gait `clip` keeps its planted foot still at `speed` metres
/// per second (#804): the speed over the clip's authored ground speed ([`crate::units::traversal::Motion::ground_speed`]),
/// within [`GAIT_RATES`], at the authored stride (Atlas 6.6.12). A clip without one keeps
/// `fallback`.
#[must_use]
pub fn gait_rate(motions: Option<&Motions>, clip: &str, speed: f64, fallback: f32) -> f32 {
    motions
        .and_then(|motions| motions.get(clip))
        .and_then(|motion| motion.ground_speed)
        .filter(|&ground| ground > 0.)
        .map_or(fallback, |ground| {
            #[allow(clippy::cast_possible_truncation)]
            let rate = (speed / ground).clamp(GAIT_RATES[0], GAIT_RATES[1]) as f32;
            rate
        })
}

/// About how fast the sword footwork's recorded travel goes (asset-tooling#757): a 0.52 m
/// sidestep in 0.5 s, a 0.6 m hop back in 0.6 s, in metres per second.
const FOOTWORK_SPEED: f64 = 1.04;

/// A skirmisher's footwork stepping `direction` from its facing at `speed` (#963): a sidestep
/// toward the side it moves, or the hop back when it backs away, paced to the travel. `None`
/// moving forward, or when the model lacks the clip: the gaits then play.
fn footwork(
    clips: Clips,
    available: &impl Fn(&str) -> bool,
    motions: Option<&Motions>,
    speed: f64,
    direction: f64,
) -> Option<Want> {
    let sword = clips.sword?;
    let angle = direction.abs();
    let clip = if angle <= STRAFE_ANGLE {
        return None;
    } else if angle >= BACKPEDAL_ANGLE {
        sword.hop_back
    } else if direction > 0. {
        sword.sidesteps[0]
    } else {
        sword.sidesteps[1]
    };
    #[allow(clippy::cast_possible_truncation)]
    let fallback = (speed / FOOTWORK_SPEED).clamp(GAIT_RATES[0], GAIT_RATES[1]) as f32;
    available(clip).then(|| Want::looped(clip, gait_rate(motions, clip, speed, fallback)))
}

/// `clip`'s upper-body twin when it is a clip of the full-body bow cycle and the model has the
/// twin (#607), else `clip` itself.
fn upper_twin(clips: Clips, available: &impl Fn(&str) -> bool, clip: &'static str) -> &'static str {
    clips
        .strikes
        .zip(clips.bow)
        .and_then(|(strikes, bow)| bow.upper_of(strikes.bow_cycle, strikes.bow, clip))
        .filter(|twin| available(twin))
        .unwrap_or(clip)
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
    /// What plays once after a once-played `then` this many seconds long (#989): the lynx's
    /// recovery after its lunge.
    last: Option<(f32, &'static str)>,
    serial: u32,
    started: f64,
    until: f64,
    region: Region,
    /// The reaction it shows, if it is one.
    reaction: Option<Reaction>,
    /// Begun standing on the ground: the authored footwork plays on the legs too (Atlas 6.6.2),
    /// until the unit moves or leaves the ground; then only the upper body carries it.
    full_body: bool,
    /// A cast's strike wound up and held at this clip time until it resolves (#582).
    hold: Option<f32>,
    /// What a throw draws from the belt (#603).
    item: Option<Throwable>,
    /// A timed swing (#679): wound up to this clip time and held until the zone's active phase
    /// begins at this time, then swung on from there.
    timed: Option<(f32, f64, f32)>,
    /// Fitted to the zone's contact tick (#900).
    fit: Option<Fit>,
    /// A jumping strike's touchdown frame: it holds there while the unit is still in the air
    /// (#900).
    land: Option<f32>,
    /// The family guard it settles into once it ends (#900).
    settle: Option<&'static str>,
}

impl Strike {
    fn new(clip: &'static str, length: f32, serial: u32, started: f64, region: Region) -> Self {
        Self {
            clip,
            length,
            then: None,
            last: None,
            serial,
            started,
            until: started + f64::from(length.max(0.3)),
            region,
            reaction: None,
            full_body: region != Region::Upper,
            hold: None,
            item: None,
            timed: None,
            fit: None,
            land: None,
            settle: None,
        }
    }

    /// The clip and pinned time to show at `now`.
    #[allow(clippy::cast_possible_truncation)]
    fn want(self, now: f64) -> Want {
        let at = match self.timed {
            Some((hold, release, _)) if now < release => {
                let startup = (release - self.started).max(1e-6);
                hold * (((now - self.started) / startup).clamp(0., 1.) as f32)
            }
            Some((hold, release, active)) => {
                // The authored slash reaches its follow-through at 68% (#447). Fit that
                // motion to the zone's whole active window, then recover at authored speed.
                let end = self.length * 0.68;
                let elapsed = (now - release).max(0.) as f32;
                hold + (end - hold).max(0.) * (elapsed / active.max(1e-6)).min(1.)
                    + (elapsed - active).max(0.)
            }
            None => {
                let elapsed = (now - self.started).max(0.) as f32;
                self.fit.map_or(elapsed, |fit| fit.at(elapsed))
            }
        };
        let at = self.hold.map_or(at, |hold| at.min(hold));
        match (self.then, self.last) {
            (Some((_, false)), Some((span, last))) if at >= self.length + span => Want {
                once: true,
                at: Some(at - self.length - span),
                ..Want::hold(last, self.serial)
            },
            (Some((next, looped)), _) if at >= self.length => Want {
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

    /// Holds the strike's clock back by `seconds`: a hitstop or a jump still in the air (#900).
    fn delay(&mut self, seconds: f64) {
        self.started += seconds;
        self.until += seconds;
        if let Some((_, release, _)) = &mut self.timed {
            *release += seconds;
        }
    }
}

/// The slowest and fastest a fitted wind-up plays its clip (#900).
pub const FIT_RATES: [f32; 2] = [0.6, 1.75];

/// A strike clip fitted to the zone's contact tick (#900): it winds up from clip time `from` to
/// its authored `contact` frame in exactly `zone` seconds, at a rate within [`FIT_RATES`], and
/// plays on at authored speed after. A contact the bounded rate would reach early holds there.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Fit {
    from: f32,
    contact: f32,
    zone: f32,
}

impl Fit {
    /// The fit of a clip whose contact frame is `contact` seconds in, to a zone contact `zone`
    /// seconds from the strike's start.
    #[must_use]
    pub fn new(contact: f32, zone: f32) -> Self {
        let (contact, zone) = (contact.max(0.), zone.max(0.));
        Self {
            from: (contact - zone * FIT_RATES[1]).max(0.),
            contact,
            zone,
        }
    }

    /// The clip time `elapsed` seconds after the strike began.
    #[must_use]
    pub fn at(self, elapsed: f32) -> f32 {
        if elapsed >= self.zone {
            self.contact + (elapsed - self.zone)
        } else {
            let rate = ((self.contact - self.from) / self.zone).max(FIT_RATES[0]);
            (self.from + elapsed * rate).min(self.contact)
        }
    }

    /// Seconds from the strike's start until a clip `length` seconds long ends.
    fn ends(self, length: f32) -> f32 {
        self.zone + (length - self.contact).max(0.)
    }
}

/// How a basic melee strike (#885) is shown (#900): fitted to the zone's contact, held on a
/// jump's touchdown frame until the unit lands, and settling into the family guard after.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Melee {
    pub fit: Option<Fit>,
    pub land: Option<f32>,
    pub settle: Option<&'static str>,
}

/// How long a strike's family guard holds after a running or jumping strike, before the body
/// eases back to its gait or idle (#900).
pub const SETTLE_SECONDS: f64 = 0.45;

/// The grounded speed in metres per second at which a strike's footwork blends in or out at
/// half its standing blend time (#900).
pub const FOOT_BLEND_SPEED: f32 = 3.;

/// The family guard a running or jumping strike of a `family` weapon settles into (#900); a
/// bow has none.
#[must_use]
pub const fn family_guard(family: WeaponFamily) -> Option<&'static str> {
    Some(match family {
        WeaponFamily::Sword => "Sword_Idle",
        WeaponFamily::Mace => "Mace_Guard_Loop",
        WeaponFamily::Dagger => "Dagger_Guard_Loop",
        WeaponFamily::Staff => "Staff_Guard_Loop",
        WeaponFamily::Spear => "Spear_Guard_Loop",
        WeaponFamily::Unarmed => "Unarmed_Guard_Loop",
        WeaponFamily::TwoHanded => "TwoHand_Guard_Loop",
        WeaponFamily::Bow => return None,
    })
}

/// The full-body standing strikes (asset-tooling #741): played on the whole skeleton while the
/// unit stands, never as an upper-body layer over the idle legs (#900).
pub const FULL_BODY_STRIKES: [&str; 14] = [
    "Sword_Regular_A",
    "Sword_Regular_B",
    "Sword_Regular_C",
    "TwoHand_Swing_A",
    "TwoHand_Swing_B",
    "TwoHand_Swing_C",
    "Spear_Thrust_A",
    "Spear_Thrust_B",
    "Mace_Swing_A",
    "Mace_Swing_B",
    "Mace_Swing_C",
    "Unarmed_Jab",
    "Unarmed_Cross",
    "Unarmed_Hook",
];

/// How far into its strike clip a local wind-up (Atlas 6.6.10) goes before it holds: this share
/// of the clip, at most [`WINDUP_HOLD_SECONDS`], so it stops short of the swing's impact.
pub const WINDUP_FRACTION: f32 = 0.3;
/// The longest a wind-up plays before holding, in seconds.
pub const WINDUP_HOLD_SECONDS: f32 = 0.3;
/// Where a directed slash's wind-up peaks and holds, in seconds (asset-tooling #447).
pub const SLASH_WINDUP_SECONDS: f32 = 0.18;
/// How far toward its `humanoid-clips` contact a strike with one winds up before holding.
pub const WINDUP_CONTACT_SHARE: f32 = 0.6;

/// Where a wind-up of a strike clip `length` seconds long holds (#582): a directed slash at
/// [`SLASH_WINDUP_SECONDS`], a clip with a `contact` time at [`WINDUP_CONTACT_SHARE`] of it,
/// any other at [`WINDUP_FRACTION`] of the clip, at most [`WINDUP_HOLD_SECONDS`].
#[must_use]
pub fn windup_seconds(length: f32, contact: Option<f32>, slash: bool) -> f32 {
    let hold = if slash {
        SLASH_WINDUP_SECONDS
    } else if let Some(contact) = contact.filter(|&contact| contact > 0.) {
        contact * WINDUP_CONTACT_SHARE
    } else {
        (length * WINDUP_FRACTION).min(WINDUP_HOLD_SECONDS)
    };
    hold.min(length.max(0.))
}

/// The local player's reversible wind-up (Atlas 6.6.10): the opening of the strike clip the
/// prepared input would play, on the upper body only and held before impact. The zone's
/// accepted strike continues it; a refusal or supersession lets it fade out.
#[derive(Clone, Copy, Debug)]
struct Preparation {
    clip: &'static str,
    serial: u32,
    started: f64,
    /// The clip time it holds at ([`windup_seconds`]).
    hold: f32,
    /// What a throw's wind-up draws from the belt (#603).
    item: Option<Throwable>,
}

impl Preparation {
    /// The wind-up's clip time at `now`.
    #[allow(clippy::cast_possible_truncation)]
    fn at(self, now: f64) -> f32 {
        ((now - self.started).max(0.) as f32).min(self.hold)
    }
}

/// One emote or interaction clip being shown (#619): upper-body over any gait, or whole-body
/// while the unit stands still and upper-body once it moves (Atlas 6.6.i745).
#[derive(Clone, Copy, Debug)]
struct Gesture {
    clip: &'static str,
    serial: u32,
    started: f64,
    until: f64,
    whole: bool,
}

/// The part of a pose's chain being shown (#619), and since when.
#[derive(Clone, Copy, Debug)]
struct PostureStep {
    posture: Posture,
    phase: PosturePhase,
    since: f64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PosturePhase {
    Enter,
    Loop,
    Exit,
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
    /// A confirmed blow's hitstop holds the strike and reaction clocks until this time (#900).
    stop: Option<f64>,
    /// The family guard a running or jumping strike settles into, until when (#900).
    settle: Option<(&'static str, f64)>,
    /// A great weapon's draw or sheathe being shown (#1073): its clip, whether it ends drawn,
    /// and its time; and how many have begun.
    readying: Option<(&'static str, bool, f32)>,
    readies: u32,
    /// The weapon mode the last frame showed.
    shown_drawn: Option<bool>,
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
    /// The charge being shown since this time (#582).
    charging: Option<f64>,
    /// Dead last frame: rising again plays the respawn (#582).
    dead: bool,
    /// How many auto-attacks the unit has swung: a twin-dagger holder alternates its hands (#603).
    swings: u32,
    /// Which of the model's varied deaths the unit falls with (#885).
    death: u8,
    /// The emote or interaction one-shot shown, on the upper body or the whole body (#619).
    gesture: Option<Gesture>,
    /// The enter, loop or exit of the pose held (#619) and how many have begun.
    posture: Option<PostureStep>,
    postures: u32,
    /// The pose was broken and stays off until its cue goes away (#619).
    posture_broken: bool,
    attitude: Attitude,
    /// Pelvis and legs: locomotion, flight and death.
    base: Track,
    /// Over the legs' locomotion: a landing or a grounded whole-body strike.
    lower: Track,
    /// Spine, arms and head over the legs' clip: a strike or the guard.
    upper: Track,
    /// Each hand's grip pose over its fingers, left then right (asset-tooling #285).
    grips: [Track; 2],
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
    /// How many auto-attacks the unit has swung ([`Kind::dagger_strike`]'s `swing`).
    #[must_use]
    pub const fn swings(&self) -> u32 {
        self.swings
    }

    /// The unit swung an auto-attack: the next one alternates the hand (#603).
    pub fn swung(&mut self) {
        self.swings = self.swings.wrapping_add(1);
    }

    /// The unit died: it falls with the `variant`-th of its model's varied deaths (#885).
    pub fn died(&mut self, variant: u8) {
        self.death = variant;
    }

    /// The strike just begun is a throw of `item` (#603): from the clip's `drawSeconds` to its
    /// `releaseSeconds` the right hand holds it, with its dagger sheathed ([`Carry`]).
    pub fn throwing(&mut self, item: Throwable) {
        if let Some(strike) = &mut self.strike {
            strike.item = Some(item);
        }
    }

    /// Starts an emote or interaction `clip`, `duration` seconds long, at `now` (#619). A
    /// `whole`-body one takes the legs only while the unit stands and otherwise plays on the
    /// upper body over the gait (Atlas 6.6.i745); anything the zone accepts for the unit, a
    /// strike, a cast or a blow, ends it at once.
    pub fn gesture(&mut self, clip: &'static str, duration: f32, whole: bool, now: f64) {
        self.serial = self.serial.wrapping_add(1);
        self.gesture = Some(Gesture {
            clip,
            serial: self.serial,
            started: now,
            until: now + f64::from(duration),
            whole,
        });
    }

    /// How many strikes, casts and hit reactions the unit has begun: a cue for cosmetic motion
    /// that follows them (Atlas 6.6.39).
    #[must_use]
    pub const fn actions_begun(&self) -> u32 {
        self.serial
    }

    /// Starts presenting an accepted strike's `clip`, `duration` seconds long, at `now`, on the
    /// body `region` drives. A strike resolves any cast shown before it; one that resolves a
    /// cast's held wind-up of the same clip (#582) swings on from the hold.
    pub fn strike(&mut self, clip: &'static str, duration: f32, region: Region, now: f64) {
        self.serial = self.serial.wrapping_add(1);
        let started = match self.cast {
            Some(Strike {
                clip: wound,
                hold: Some(hold),
                ..
            }) if wound == clip => now - f64::from(hold),
            _ => now,
        };
        self.strike = Some(Strike::new(clip, duration, self.serial, started, region));
        self.cast = None;
    }

    /// The zone accepted the prepared strike: it continues from the wind-up's phase when the
    /// wind-up shows the same clip, so the swing commits without restarting. Otherwise it is an
    /// ordinary [`Self::strike`] and the wind-up ends.
    pub fn commit(&mut self, clip: &'static str, duration: f32, region: Region, now: f64) {
        match self.preparation.take() {
            Some(preparation) if preparation.clip == clip => {
                let started = now - f64::from(preparation.at(now));
                self.strike = Some(Strike::new(
                    clip,
                    duration,
                    preparation.serial,
                    started,
                    region,
                ));
                self.cast = None;
            }
            _ => self.strike(clip, duration, region, now),
        }
    }

    /// Shows a timed swing (#679) of `clip`, `duration` seconds long, begun at `now`: it winds
    /// up to `hold` over `wind_up`, fits the cut to `active`, then recovers at authored speed.
    /// A prepared wind-up of the same clip continues.
    pub fn timed(
        &mut self,
        clip: &'static str,
        duration: f32,
        hold: f32,
        region: Region,
        [wind_up, active]: [f32; 2],
        now: f64,
    ) {
        let (serial, started) = match self.preparation.take() {
            Some(preparation) if preparation.clip == clip => {
                (preparation.serial, preparation.started)
            }
            _ => {
                self.serial = self.serial.wrapping_add(1);
                (self.serial, now)
            }
        };
        let hold = hold.min(duration.max(0.));
        let release = now + f64::from(wind_up.max(0.));
        let mut strike = Strike::new(clip, duration, serial, started, region);
        strike.timed = Some((hold, release, active));
        strike.until = release + f64::from(active + (duration * 0.32).max(0.12));
        self.strike = Some(strike);
        self.cast = None;
    }

    /// Shows a basic melee strike (#885) of `clip`, `duration` seconds long, begun at `now`, as
    /// `melee` shapes it (#900): its contact frame fitted to the zone's, a jump's touchdown
    /// held until the unit lands, and the family guard after it.
    pub fn melee(
        &mut self,
        clip: &'static str,
        duration: f32,
        region: Region,
        melee: Melee,
        now: f64,
    ) {
        self.strike(clip, duration, region, now);
        self.settle = None;
        if let Some(strike) = &mut self.strike {
            strike.fit = melee.fit;
            strike.land = melee.land.filter(|&land| land < duration);
            strike.settle = melee.settle;
            if let Some(fit) = melee.fit {
                strike.until = strike.started + f64::from(fit.ends(duration).max(0.3));
            }
        }
    }

    /// A blow this unit struck or took was confirmed at `now` (#900): its strike or reaction
    /// holds for `seconds` at [`crate::units::contact::HITSTOP_RATE`] of its pace. The zone's timing is
    /// unchanged; reduced motion ignores it.
    pub fn hitstop(&mut self, seconds: f32, now: f64) {
        let until = now + f64::from(seconds.max(0.));
        self.stop = Some(self.stop.map_or(until, |stop| stop.max(until)));
    }

    /// The strike (not a reaction) the unit is swinging at `now`, if any.
    #[must_use]
    pub fn striking(&self, now: f64) -> Option<&'static str> {
        self.strike
            .filter(|strike| strike.reaction.is_none() && now < strike.until)
            .map(|strike| strike.clip)
    }

    /// Shows a cast of `seconds` begun at `now` that resolves into a strike of `clip` (#582):
    /// the Redbrand whirl and smash wind up, then hold at `hold` seconds into the clip until the
    /// zone resolves the strike ([`Self::strike`] swings on from there) or the cast stops.
    pub fn wind_up(
        &mut self,
        clip: &'static str,
        duration: f32,
        hold: f32,
        region: Region,
        seconds: f32,
        now: f64,
    ) {
        self.serial = self.serial.wrapping_add(1);
        let mut cast = Strike::new(clip, duration, self.serial, now, region);
        cast.hold = Some(hold);
        // Held a little past the cast, so a resolution arriving late still swings on.
        cast.until = now + f64::from(seconds.max(0.3) + WIND_UP_SLACK_SECONDS);
        self.cast = Some(cast);
    }

    /// The zone carries the unit on a charge from `now` (#582): it rushes with the charge loop
    /// until it arrives, then plays the shield impact.
    pub fn charge(&mut self, now: f64) {
        self.charging = Some(now);
    }

    /// Whether the unit shows a charge's rush.
    #[must_use]
    pub const fn charging(&self) -> bool {
        self.charging.is_some()
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

    /// The strike just begun goes on through `then` and then `last`, each a clip and its
    /// seconds (#989): the lynx's wind-up into its lunge and its recovery.
    pub fn recover_through(&mut self, then: (&'static str, f32), last: (&'static str, f32)) {
        self.recover(then.0, then.1);
        if let Some(strike) = &mut self.strike
            && strike.then.is_some()
            && last.1 > 0.
        {
            strike.last = Some((then.1, last.0));
            strike.until += f64::from(last.1);
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

    /// Shows a cast or channel of `seconds` begun at `now` on the body `region` drives (#538):
    /// `enter` once for its length, when the model has it, then `then`'s clip, looped or once,
    /// until the cast resolves, stops or runs out. A caster's charge or channel loops (#609);
    /// Raise Skeleton's kneel plays once from the start, Call Companion's whistle after the
    /// charge.
    pub fn cast(
        &mut self,
        enter: Option<(&'static str, f32)>,
        then: (&'static str, bool),
        region: Region,
        seconds: f32,
        now: f64,
    ) {
        self.serial = self.serial.wrapping_add(1);
        let (clip, length) = enter.unwrap_or((then.0, 0.));
        let mut cast = Strike::new(clip, length, self.serial, now, region);
        cast.then = Some(then);
        cast.until = now + f64::from(seconds.max(0.3));
        self.cast = Some(cast);
    }

    /// The cast shown stopped early (#538): it lets down with `cancel` when the model has it,
    /// on the upper body alone when `upper` (a caster's interrupt, #609), else on the body the
    /// cast drove.
    pub fn cancel(&mut self, cancel: Option<(&'static str, f32)>, upper: bool, now: f64) {
        let Some(cast) = self.cast.take().filter(|cast| now < cast.until) else {
            return;
        };
        // A held strike wind-up (#582) only fades.
        if let Some((clip, duration)) = cancel.filter(|_| cast.hold.is_none()) {
            self.serial = self.serial.wrapping_add(1);
            let region = if upper { Region::Upper } else { cast.region };
            self.strike = Some(Strike::new(clip, duration, self.serial, now, region));
        }
    }

    /// Starts a local wind-up of `clip` begun at `started` that holds `hold` seconds into the
    /// clip ([`windup_seconds`]), or keeps the one showing it; a different clip supersedes the
    /// wind-up shown. A throw's wind-up draws `item` (#603).
    pub fn prepare(
        &mut self,
        clip: &'static str,
        hold: f32,
        item: Option<Throwable>,
        started: f64,
    ) {
        if let Some(preparation) = &mut self.preparation
            && preparation.clip == clip
        {
            preparation.item = item;
            return;
        }
        self.serial = self.serial.wrapping_add(1);
        self.preparation = Some(Preparation {
            clip,
            serial: self.serial,
            started,
            hold,
            item,
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
    fn climb(clips: Clips, duration: &impl Fn(&str) -> f32, cues: Cues<'_>) -> Want {
        let available = |clip: &str| duration(clip) > 0.;
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
        let want = Want::looped(clip, rate);
        // On a ladder the loop's phase follows the feet up the rung grid (#621): one loop climbs
        // `climbSpeedMetersPerSecond` × its length (one rung), so every hand and foot plant
        // lands on a rung whatever the zone's climbing speed.
        let length = duration(clip);
        match cues.rungs {
            Some(offset) if cues.traversal == TraversalView::ClimbingLadder && length > 0. => {
                let cycle = authored * f64::from(length);
                let travelled = if vertical > 0. {
                    offset - LADDER_UP_RUNG_OFFSET
                } else {
                    LADDER_DOWN_RUNG_OFFSET - offset
                };
                #[allow(clippy::cast_possible_truncation)]
                let at = ((travelled / cycle).rem_euclid(1.) * f64::from(length)) as f32;
                Want {
                    at: Some(at),
                    ..want
                }
            }
            _ => want,
        }
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
    /// otherwise. Each plays at [`gait_rate`], so its planted foot keeps pace with the ground.
    /// A `great` weapon's carrying run (#1073) takes the forward run and sprint.
    fn gait(
        &self,
        clips: Clips,
        available: &impl Fn(&str) -> bool,
        motions: Option<&Motions>,
        (speed, direction): (f64, f64),
        great: Option<GreatWeapon>,
    ) -> Want {
        let gait = |pick: fn(GaitClips) -> &'static str| {
            clips.gaits.map(pick).filter(|clip| available(clip))
        };
        let angle = direction.abs();
        let left = direction > 0.;
        #[allow(clippy::cast_possible_truncation)]
        let run_rate = (speed / RUN_CLIP_GROUND).clamp(0.7, 1.5) as f32;
        let great = great.map(GreatWeapon::run).filter(|clip| available(clip));
        let run = great
            .or_else(|| gait(|gaits| gaits.run))
            .unwrap_or(clips.run);
        let (clip, fallback) = if speed > RUN_CLIP_SPEED {
            let directed = if angle <= STRAFE_ANGLE {
                if speed > SPRINT_CLIP_SPEED
                    && great.is_none()
                    && let Some(sprint) = gait(|gaits| gaits.sprint)
                {
                    #[allow(clippy::cast_possible_truncation)]
                    let rate = (speed / SPRINT_CLIP_GROUND).clamp(0.7, 1.5) as f32;
                    return Want::looped(sprint, gait_rate(motions, sprint, speed, rate));
                }
                Some(run)
            } else if angle >= BACKPEDAL_ANGLE {
                gait(|gaits| gaits.run_back)
            } else if left {
                gait(|gaits| gaits.run_left)
            } else {
                gait(|gaits| gaits.run_right)
            };
            match directed {
                Some(clip) => (clip, run_rate),
                // Running forward or strafing at the player's run speed plays the run (#515);
                // backpedalling keeps the walk.
                None if angle <= FRAC_PI_2 => (run, run_rate),
                None => (clips.walk, walk_rate(speed)),
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
            (directed.unwrap_or(clips.walk), walk_rate(speed))
        };
        Want::looped(clip, gait_rate(motions, clip, speed, fallback))
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
        // A shield-bearer pivots in its guard (#953): its planted turns are an eighth turn each.
        let pivot = clips
            .sword
            .filter(|_| cues.shield_guard)
            .map(|sword| sword.shield_turns[usize::from(!left)])
            .filter(|&clip| duration(clip) > 0.);
        let (clip, radians) = match pivot {
            Some(clip) => (clip, SHIELD_TURN_CLIP_RADIANS),
            None if left => (gaits.turn_left, TURN_CLIP_RADIANS),
            None => (gaits.turn_right, TURN_CLIP_RADIANS),
        };
        let length = f64::from(duration(clip));
        if length <= 0. {
            return None;
        }
        // Through the hold the step keeps at least the threshold's pace.
        #[allow(clippy::cast_possible_truncation)]
        let rate = (cues.turn.abs().max(TURN_CLIP_RATE) * length / radians).clamp(0.7, 1.6) as f32;
        Some(Want::looped(clip, rate))
    }

    /// Advances the clocks and returns the layers to pose this frame; `duration` gives a clip's
    /// length in seconds (zero when the model lacks it).
    ///
    /// The steps run in a fixed order, each settling what the later ones read: the rise and the
    /// charge, the traversal and flight, the strike clocks, the poses and gestures, then the
    /// clips each region wants, the tracks' blends and the carried items' grips.
    pub fn advance(
        &mut self,
        clips: Clips,
        duration: impl Fn(&str) -> f32,
        cues: Cues<'_>,
        blend: Blend,
    ) -> Animation {
        #[allow(clippy::cast_possible_truncation)]
        let dt = cues.dt as f32;
        let frame = Frame {
            clips,
            duration: &duration,
            cues,
            blend: blend.clamped(),
            dt,
            guarding: cues.guard.is_some(),
        };
        let (charge_run, stun) = self.step_rise_and_charge(&frame);
        let (swims, held, traversing) = self.step_traversal(&frame);
        let stopped = self.step_strike(&frame, stun);
        self.step_readying(&frame);
        let footing = Footing {
            charge_run,
            stun,
            swims,
            held,
            traversing,
            stopped,
            // A swimmer, a climber or a mantler has no footing for a strike's footwork.
            standing: matches!(self.flight, Flight::Ground)
                && cues.speed <= 0.25
                && !swims
                && !traversing,
            moving: cues.speed > 0.25,
            layered: clips.upper_body.is_some(),
        };
        let poses = self.step_poses(&frame, &footing);
        let shown = self.strike_wants(&frame, &footing);
        let (ground, chore) = self.ground_want(&frame, &footing, &poses);
        let (roll, base) = self.base_want(&frame, &footing, ground);
        let (lower, upper) = self.region_wants(&frame, &footing, &poses, &shown, roll, base);
        let (first, held_dt) = self.step_legs(&frame, &footing, base, lower);
        self.step_attitude(&frame, roll.is_some(), poses.turned, first);
        self.step_upper(&frame, upper, shown.split, held_dt);
        let mut layers = self.compose_layers(&frame);
        let carry = self.step_carry(&frame, chore);
        let lower = self
            .lower
            .target(&duration)
            .or_else(|| self.base.target(&duration))
            .unwrap_or((clips.idle, 0.));
        let upper = self.upper.target(&duration).unwrap_or(lower);
        self.step_grips(&frame, &carry, upper.0, first, &mut layers);
        let channel = cues
            .motions
            .and_then(|motions| motions.get(upper.0))
            .and_then(|motion| motion.cast)
            .and_then(|cast| cast.channel);
        Animation {
            layers,
            lower,
            upper,
            attitude: self.attitude,
            carry,
            channel,
        }
    }

    /// The frame's first step: back from death the unit rises, and a charge ends where the zone
    /// stops carrying it. Returns the charge loop still running and the stun held.
    fn step_rise_and_charge<D: Fn(&str) -> f32>(
        &mut self,
        frame: &Frame<'_, D>,
    ) -> (Option<&'static str>, Option<&'static str>) {
        let Cues {
            speed,
            dead,
            now,
            reduced,
            ..
        } = frame.cues;
        let duration = frame.duration;
        // Back from death (#582): the unit rises from a kneel.
        if self.dead
            && !dead
            && !reduced
            && let Some(rise) = frame.sword(|sword| sword.respawn)
        {
            self.serial = self.serial.wrapping_add(1);
            self.strike = Some(Strike::new(
                rise,
                duration(rise),
                self.serial,
                now,
                Region::Footwork,
            ));
        }
        self.dead = dead;
        // A charge (#582) rushes until the zone stops carrying the unit, then strikes with the
        // shield; the zone alone decides where it ends.
        if let Some(started) = self.charging {
            let elapsed = now - started;
            if dead {
                self.charging = None;
            } else if elapsed >= CHARGE_SECONDS
                || elapsed >= CHARGE_ARRIVAL_SECONDS && speed < CHARGE_ARRIVAL_SPEED
            {
                self.charging = None;
                if let Some(impact) = frame
                    .sword(|sword| sword.charge_impact)
                    .filter(|_| !reduced)
                {
                    self.serial = self.serial.wrapping_add(1);
                    self.strike = Some(Strike::new(
                        impact,
                        duration(impact),
                        self.serial,
                        now,
                        Region::Whole,
                    ));
                }
            }
        }
        let charge_run = frame
            .sword(|sword| sword.charge_run)
            .filter(|_| self.charging.is_some());
        let stun = frame
            .sword(|sword| sword.stun)
            .filter(|_| frame.cues.stunned && !dead);
        (charge_run, stun)
    }

    /// The water crossing, the zone's climb or mantle and the flight phase. Returns whether the
    /// unit swims, the traversal it holds and whether it is traversing.
    fn step_traversal<D: Fn(&str) -> f32>(
        &mut self,
        frame: &Frame<'_, D>,
    ) -> (bool, TraversalView, bool) {
        let Frame {
            clips,
            duration,
            cues,
            ..
        } = *frame;
        let Cues {
            speed,
            vertical_speed,
            dead,
            now,
            reduced,
            ..
        } = cues;
        let landed = clips.jump.is_none_or(|jump| {
            self.lower
                .playing(jump.land)
                .is_none_or(|play| play.time >= duration(jump.land))
        });
        // The zone's water mode, never the rendered water, decides the swim clips (Atlas 6.6.41).
        let swims = cues.water.swims() && !dead;
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
                frame.swim(|clips| clips.enter)
            } else {
                frame.swim(|clips| clips.exit)
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
            self.mantle = self.begin_mantle(clips, duration, cues, ledge);
        }
        if self.held.climbing() && !held.holds() {
            self.pulls = self.pulls.wrapping_add(1);
            self.letting_go = clips
                .climb
                .map(|climb| (climb.let_go, now))
                .filter(|&(clip, _)| !reduced && frame.available(clip));
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
        (swims, held, traversing)
    }

    /// The strike and cast clocks: a guard ends them, a hitstop holds them, and a finished
    /// running or jumping strike settles into its family guard. Returns whether a hitstop holds.
    fn step_strike<D: Fn(&str) -> f32>(
        &mut self,
        frame: &Frame<'_, D>,
        stun: Option<&'static str>,
    ) -> bool {
        let Frame { dt, guarding, .. } = *frame;
        let Cues {
            dead, now, reduced, ..
        } = frame.cues;
        // A guard the zone starts protecting ends the strike shown before it; a strike accepted
        // while it protects still plays over it.
        if dead || guarding && !self.guarding {
            self.strike = None;
            self.cast = None;
            self.preparation = None;
            self.settle = None;
        }
        self.guarding = guarding;
        // A confirmed blow's hitstop (#900) holds the strike and reaction clocks for a moment; a
        // jumping strike holds its touchdown frame while the unit is still in the air.
        self.stop = self.stop.filter(|&until| now < until);
        let stopped = !reduced && self.stop.is_some();
        let stopping = if stopped {
            f64::from(dt * (1. - crate::units::contact::HITSTOP_RATE))
        } else {
            0.
        };
        let aloft = matches!(
            self.flight,
            Flight::Rising | Flight::Airborne | Flight::Falling
        );
        for strike in [&mut self.strike, &mut self.cast].into_iter().flatten() {
            let landing = aloft
                && strike
                    .land
                    .is_some_and(|land| strike.want(now).at.is_some_and(|at| at >= land));
            let hold = if landing { f64::from(dt) } else { stopping };
            if hold > 0. {
                strike.delay(hold);
            }
        }
        if let Some(preparation) = &mut self.preparation {
            preparation.started += stopping;
        }
        // A running or jumping strike settles into its family guard (#900) before the gait or
        // idle takes the body back.
        if let Some(strike) = self.strike.filter(|strike| now >= strike.until) {
            self.settle = strike
                .settle
                .filter(|&clip| !reduced && frame.available(clip))
                .map(|clip| (clip, now + SETTLE_SECONDS));
        }
        self.settle = self
            .settle
            .filter(|&(_, until)| now < until && !dead && stun.is_none());
        self.strike = self.strike.filter(|strike| now < strike.until);
        self.cast = self.cast.filter(|cast| now < cast.until);
        stopped
    }

    /// Emotes, interactions and held poses (#619), and the talking idle: the zone's own actions
    /// and blows end them at once.
    fn step_poses<D: Fn(&str) -> f32>(&mut self, frame: &Frame<'_, D>, footing: &Footing) -> Poses {
        let Frame {
            clips,
            cues,
            guarding,
            ..
        } = *frame;
        let Cues {
            dead, now, reduced, ..
        } = cues;
        let Footing {
            charge_run,
            stun,
            standing,
            moving,
            layered,
            ..
        } = *footing;
        // A whole-body one (the bow) takes the legs only while the unit stands; on the move it
        // plays on the upper body over the gait (Atlas 6.6.i745), so it never holds the legs
        // still against the zone's projected motion.
        let acting = self.strike.is_some() || self.cast.is_some();
        if self.gesture.is_some_and(|gesture| {
            now >= gesture.until || dead || acting || guarding || stun.is_some()
        }) {
            self.gesture = None;
        }
        let gesture_want = self.gesture.filter(|_| !reduced && layered).map(|gesture| {
            #[allow(clippy::cast_possible_truncation)]
            let at = (now - gesture.started).max(0.) as f32;
            (
                gesture.whole && standing,
                Want {
                    at: Some(at),
                    ..Want::hold(gesture.clip, gesture.serial)
                },
            )
        });
        let blocked = !standing
            || moving
            || dead
            || acting
            || guarding
            || cues.rolling
            || stun.is_some()
            || charge_run.is_some();
        let posture_want = self.step_posture(
            clips.social,
            cues.posture,
            blocked,
            moving,
            frame.duration,
            (now, reduced),
        );
        let occupied = posture_want.is_some() || gesture_want.is_some_and(|(whole, _)| whole);
        let talk_want = clips
            .social
            .map(|social| social.talk_idle)
            .filter(|&clip| cues.talking && !dead && layered && frame.available(clip))
            .map(|clip| Want::looped(clip, 1.));
        let turned = self.posture.is_some()
            || self.gesture.is_some_and(|gesture| {
                clips.social.is_some_and(|social| {
                    gesture.clip == social.point || gesture.clip == social.talk_direct
                })
            });
        Poses {
            gesture: gesture_want,
            posture: posture_want,
            occupied,
            talk: talk_want,
            turned,
        }
    }

    /// The strike or cast shown and the local wind-up under it; a strike that leaves the legs
    /// mid-swing splits onto the upper body.
    fn strike_wants<D: Fn(&str) -> f32>(
        &mut self,
        frame: &Frame<'_, D>,
        footing: &Footing,
    ) -> Shown {
        let Cues { now, reduced, .. } = frame.cues;
        let Footing {
            standing, layered, ..
        } = *footing;
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
                strike.is_none() && !reduced && layered && frame.available(preparation.clip)
            })
            .map(|preparation| Want {
                // The pinned time is the wind-up's clock; a still rate keeps an abandoned wind-up
                // from playing on toward the impact while it fades out.
                rate: 0.,
                at: Some(preparation.at(now)),
                ..Want::hold(preparation.clip, preparation.serial)
            });
        Shown {
            strike,
            prepared,
            split,
        }
    }

    /// The legs' clip on the ground: a pose, the traversal, the charge, a stun, the water, the
    /// gait, a standing turn, a chore or the idle. Returns it with the chore shown, if any.
    fn ground_want<D: Fn(&str) -> f32>(
        &mut self,
        frame: &Frame<'_, D>,
        footing: &Footing,
        poses: &Poses,
    ) -> (Want, Option<Chore>) {
        let Frame {
            clips,
            duration,
            cues,
            guarding,
            ..
        } = *frame;
        let Cues {
            speed,
            vertical_speed,
            dead,
            ..
        } = cues;
        let Footing {
            charge_run,
            stun,
            swims,
            held,
            ..
        } = *footing;
        let available = |clip: &str| frame.available(clip);
        // A resident's chore (#904) shows its item only while it stands or walks; a chore clip
        // the figure lacks leaves the hands empty and the ordinary clips playing.
        let chore = cues.chore.filter(|chore| {
            !dead
                && [chore.idle, chore.walk]
                    .into_iter()
                    .flatten()
                    .all(&available)
        });
        let mut chored = false;
        let ground = if let Some(pose) = poses.posture.or_else(|| {
            poses
                .gesture
                .filter(|&(whole, _)| whole)
                .map(|(_, want)| want)
        }) {
            self.turning = None;
            pose
        } else if let Some((clip, rate)) = self.mantle {
            Want {
                rate,
                ..Want::hold(clip, self.pulls)
            }
        } else if held.climbing() {
            self.turning = None;
            Self::climb(clips, duration, cues)
        } else if held.mantling().is_some() {
            // No mantle clip: the earlier in-place fallback, the walk at the mantle's pace.
            let travel = speed.hypot(vertical_speed);
            if travel > 0.25 {
                Want::looped(clips.walk, walk_rate(travel))
            } else {
                Want::looped(clips.idle, 1.)
            }
        } else if let Some(run) = charge_run {
            self.turning = None;
            #[allow(clippy::cast_possible_truncation)]
            let rate = (speed / CHARGE_CLIP_SPEED).clamp(0.8, 1.6) as f32;
            Want::looped(run, rate)
        } else if let Some(stun) = stun {
            self.turning = None;
            Want::looped(stun, 1.)
        } else if swims {
            // Strokes keep pace with the projected travel; a missing clip falls back to the
            // leaned walk for travel and the idle for treading, in place either way.
            let travel = speed.hypot(vertical_speed);
            #[allow(clippy::cast_possible_truncation)]
            let stroke_rate = (travel / STROKE_SPEED).clamp(0.6, 1.5) as f32;
            let vertical = if vertical_speed < -DIVE_SPEED {
                frame.swim(|clips| clips.descend)
            } else if vertical_speed > DIVE_SPEED {
                frame.swim(|clips| clips.ascend)
            } else {
                None
            };
            if let Some(clip) = vertical {
                Want::looped(clip, stroke_rate)
            } else if travel > 0.25 {
                match frame.swim(|clips| clips.stroke) {
                    Some(stroke) => Want::looped(stroke, stroke_rate),
                    None => Want::looped(clips.walk, walk_rate(travel)),
                }
            } else {
                Want::looped(frame.swim(|clips| clips.tread).unwrap_or(clips.idle), 1.)
            }
        } else if let Some(wade) = frame.swim(|clips| clips.wade)
            && cues.water == WaterMode::Wading
            && speed > 0.25
        {
            Want::looped(wade, gait_rate(cues.motions, wade, speed, walk_rate(speed)))
        } else if speed > 0.25 {
            self.turning = None;
            chored = chore.is_some();
            // A load carried at run speed (#1073) runs on the heavy legs; the upper body hugs
            // the load over them ([`Self::region_wants`]).
            let loaded_run = chore.is_some_and(Chore::load)
                && speed > RUN_CLIP_SPEED
                && footing.layered
                && available(HEAVY_RUN)
                && available(CARRY_RUN);
            #[allow(clippy::cast_possible_truncation)]
            let run_rate = (speed / RUN_CLIP_GROUND).clamp(0.7, 1.5) as f32;
            match chore.and_then(|chore| chore.walk) {
                _ if loaded_run => Want::looped(
                    HEAVY_RUN,
                    gait_rate(cues.motions, HEAVY_RUN, speed, run_rate),
                ),
                Some(walk) => {
                    Want::looped(walk, gait_rate(cues.motions, walk, speed, walk_rate(speed)))
                }
                None => cues
                    .footwork
                    .then(|| footwork(clips, &available, cues.motions, speed, cues.direction))
                    .flatten()
                    .unwrap_or_else(|| {
                        let great = cues.loadout.and_then(Loadout::great_weapon);
                        self.gait(
                            clips,
                            &available,
                            cues.motions,
                            (speed, cues.direction),
                            great,
                        )
                    }),
            }
        } else if let Some(turn) = self.turn(clips, duration, cues, guarding) {
            chored = chore.is_some();
            turn
        } else if let Some(chore) = chore.filter(|_| !guarding) {
            chored = true;
            Want::looped(chore.idle.unwrap_or(clips.idle), 1.)
        } else {
            // A standing protecting guard holds the shield up with its own stance; a fighting
            // shield-bearer stands in its whole-body guard (#953).
            // A creature at rest out of a fight holds its rest pose: the bat its roost (#995), the
            // stag grazes (#997).
            let rest = clips
                .rest
                .filter(|&clip| !cues.engaged && available(clip))
                .unwrap_or(clips.idle);
            let idle = frame
                .sword(|sword| sword.shield_guard)
                .filter(|_| cues.shield_guard)
                .unwrap_or(rest);
            Want::looped(clips.guard.filter(|_| guarding).unwrap_or(idle), 1.)
        };
        (ground, chore.filter(|_| chored))
    }

    /// The pelvis and legs' clip: the death, a dodge roll, the jump or the ground's clip.
    /// Returns it with the roll clip shown, if any.
    fn base_want<D: Fn(&str) -> f32>(
        &mut self,
        frame: &Frame<'_, D>,
        footing: &Footing,
        ground: Want,
    ) -> (Option<&'static str>, Want) {
        let Frame {
            clips, duration, ..
        } = *frame;
        let Cues {
            dead,
            reduced,
            rolling,
            ..
        } = frame.cues;
        // A dodge roll (#516) plays its clip once over the roll's length, whatever the legs did.
        if rolling && !self.rolling {
            self.rolls = self.rolls.wrapping_add(1);
        }
        self.rolling = rolling;
        let roll = clips.roll.filter(|&clip| {
            rolling && !dead && !footing.swims && !reduced && frame.available(clip)
        });
        let base = if dead {
            let varied = clips
                .deaths
                .map(|deaths| deaths[usize::from(self.death) % deaths.len()])
                .filter(|&clip| frame.available(clip));
            Want::hold(varied.unwrap_or(clips.death), 0)
        } else if let Some(clip) = roll {
            #[allow(clippy::cast_possible_truncation)]
            let rate = (f64::from(duration(clip)) / ROLL_SECONDS) as f32;
            Want {
                rate,
                ..Want::hold(clip, self.rolls)
            }
        } else {
            match (clips.jump, self.flight) {
                (Some(jump), Flight::Rising) if !reduced && frame.available(jump.start) => {
                    Want::hold(jump.start, self.landing)
                }
                (Some(jump), Flight::Rising | Flight::Airborne | Flight::Falling)
                    if frame.available(jump.airborne) =>
                {
                    Want::looped(jump.airborne, 1.)
                }
                _ => ground,
            }
        };
        (roll, base)
    }

    /// What plays over the legs' clip on the lower body (a whole-body strike, the settling
    /// guard, a landing, a let-go or a water crossing) and on the upper body (a strike, the
    /// wind-up, a guard, a gesture, the talk idle or the weapon's stance).
    fn region_wants<D: Fn(&str) -> f32>(
        &self,
        frame: &Frame<'_, D>,
        footing: &Footing,
        poses: &Poses,
        shown: &Shown,
        roll: Option<&'static str>,
        base: Want,
    ) -> (Option<Want>, Option<Want>) {
        let Frame {
            clips,
            cues,
            guarding,
            ..
        } = *frame;
        let Cues {
            speed,
            dead,
            reduced,
            ..
        } = cues;
        let Footing {
            charge_run,
            stun,
            swims,
            traversing,
            standing,
            layered,
            ..
        } = *footing;
        let Shown {
            strike, prepared, ..
        } = *shown;
        let available = |clip: &str| frame.available(clip);
        // The settling guard (#900) takes the whole body on the ground, the upper body over a
        // gait; on touchdown it stands in for the jump's landing.
        let settle = self
            .settle
            .filter(|_| strike.is_none() && roll.is_none())
            .map(|(clip, _)| {
                let planted = speed <= 0.25
                    && !swims
                    && !traversing
                    && matches!(self.flight, Flight::Ground | Flight::Landing);
                (planted, Want::looped(clip, 1.))
            });
        let lower = if dead || roll.is_some() {
            None
        } else if let Some((true, want)) = strike.or(settle) {
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
        } else if let Some((false, want)) = strike.or(settle) {
            Some(want)
        } else if prepared.is_some() {
            prepared
        } else if let Some((clip, ..)) = self.readying {
            // A great weapon drawn from or put across the back (#1073).
            Some(Want::hold(clip, self.readies))
        } else if let Some(lane) = cues
            .guard
            .and_then(|lane| clips.sword.map(|sword| sword.guard(lane)))
            .filter(|clip| available(clip))
        {
            // The raised shield in the zone's guard lane (#582).
            Some(Want::looped(lane, 1.))
        } else if let Some(guard) = clips.guard
            && guarding
            && base.clip != guard
            && available(guard)
        {
            Some(Want::looped(guard, 1.))
        } else if let Some(clip) = cues
            .rooted
            .zip(clips.reactions.and_then(|reactions| reactions.rooted))
            .map(|(hold, [roots, ice])| {
                if hold == crate::units::effects::Condition::Frost {
                    ice
                } else {
                    roots
                }
            })
            .filter(|&clip| stun.is_none() && available(clip))
        {
            // The upper body strains against the root that holds it (#956).
            Some(Want::looped(clip, 1.))
        } else if let Some((false, want)) = poses.gesture {
            Some(want)
        } else if poses.occupied {
            None
        } else if let Some(want) = poses.talk {
            Some(want)
        } else if base.clip == HEAVY_RUN && available(CARRY_RUN) {
            // A load carried at run speed (#1073): the upper body hugs it over the heavy legs,
            // in the stride's phase.
            let stride = (frame.duration)(HEAVY_RUN);
            let phase = self
                .base
                .playing(HEAVY_RUN)
                .filter(|_| stride > 0.)
                .map_or(0., |play| {
                    ((play.time + frame.dt * play.rate) / stride).rem_euclid(1.)
                });
            Some(Want {
                at: Some(phase * (frame.duration)(CARRY_RUN)),
                ..Want::looped(CARRY_RUN, base.rate)
            })
        } else if let Some(crossbow) = clips.crossbow.filter(|_| {
            cues.loadout == Some(Loadout::Crossbow)
                && stun.is_none()
                && roll.is_none()
                && !swims
                && !traversing
        }) {
            // A crossbowman carries the crossbow low, standing or on the move, and aims it
            // standing in combat (#607).
            let stance = if standing && cues.engaged {
                crossbow.aim
            } else {
                crossbow.idle
            };
            Some(stance)
                .filter(|&clip| available(clip))
                .map(|stance| Want::looped(stance, 1.))
        } else {
            // A dagger holder stands in the low dagger stance (#603) over the idle legs, a great
            // weapon's holder with it drawn in both hands (#1073).
            let great = cues
                .loadout
                .and_then(Loadout::great_weapon)
                .filter(|_| cues.weapon_drawn != Some(false))
                .map(|_| GREAT_WEAPON_IDLE);
            clips
                .dagger
                .map(|dagger| dagger.idle)
                .filter(|_| cues.loadout.is_some_and(Loadout::daggers))
                .or(great)
                .filter(|&clip| {
                    standing
                        && stun.is_none()
                        && charge_run.is_none()
                        && roll.is_none()
                        && available(clip)
                })
                .map(|stance| Want::looped(stance, 1.))
        };
        // The bow cycle on the upper body alone plays its upper-body twin (#607): the hips stay
        // square over whatever the legs play.
        let upper = upper.map(|want| Want {
            clip: upper_twin(clips, &available, want.clip),
            ..want
        });
        (lower, upper)
    }

    /// Steps the legs' tracks toward `base` and the `lower` clip over it. Returns whether this
    /// is the unit's first frame and the strike regions' step, slowed by a hitstop.
    fn step_legs<D: Fn(&str) -> f32>(
        &mut self,
        frame: &Frame<'_, D>,
        footing: &Footing,
        base: Want,
        lower: Option<Want>,
    ) -> (bool, f32) {
        let Frame { blend, dt, .. } = *frame;
        let Cues {
            speed,
            now,
            reduced,
            ..
        } = frame.cues;
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
        // Entering or leaving a strike's footwork blends faster the faster the unit travels
        // (#900), so a planted stance is never smeared over ground the body has already crossed.
        #[allow(clippy::cast_possible_truncation)]
        let over_seconds = lower_seconds / (1. + speed as f32 / FOOT_BLEND_SPEED);
        // A hitstop (#900) slows the strike's regions with the clocks it holds.
        let held_dt = if footing.stopped {
            dt * crate::units::contact::HITSTOP_RATE
        } else {
            dt
        };
        self.lower.advance(lower, held_dt, over_seconds, reduced);
        (first, held_dt)
    }

    /// Settles the body's swim lean and turn, or turns it with a roll.
    fn step_attitude<D: Fn(&str) -> f32>(
        &mut self,
        frame: &Frame<'_, D>,
        rolling: bool,
        turned: bool,
        first: bool,
    ) {
        let cues = frame.cues;
        let Cues {
            speed,
            vertical_speed,
            dead,
            reduced,
            ..
        } = cues;
        if rolling {
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
            let attitude = Attitude {
                heading: cues.face.filter(|_| turned).unwrap_or(attitude.heading),
                ..attitude
            };
            self.attitude = self.attitude.settle(attitude, cues.dt, reduced || first);
        }
    }

    /// Steps the upper body's track toward `upper`.
    fn step_upper<D: Fn(&str) -> f32>(
        &mut self,
        frame: &Frame<'_, D>,
        upper: Option<Want>,
        split: bool,
        held_dt: f32,
    ) {
        let reduced = frame.cues.reduced;
        if split {
            // The strike leaves the legs mid-swing: the upper body keeps it without a blend.
            self.upper = Track::default();
            self.upper.advance(upper, held_dt, 0., reduced);
        } else {
            self.upper
                .advance(upper, held_dt, frame.blend.upper_seconds, reduced);
        }
    }

    /// The body's layers: the legs' clips, and the upper body's over them or following them.
    fn compose_layers<D: Fn(&str) -> f32>(&self, frame: &Frame<'_, D>) -> Vec<Layer> {
        let Frame {
            duration, blend, ..
        } = *frame;
        let mut over = Vec::new();
        self.lower
            .layers(duration, blend.lower_weight, Mask::Lower, &mut over);
        let over_weight: f32 = over.iter().map(|layer| layer.weight).sum();
        let mut legs = Vec::new();
        self.base
            .layers(duration, 1. - over_weight, Mask::Lower, &mut legs);
        legs.extend(over);
        let mut arms = Vec::new();
        self.upper
            .layers(duration, blend.upper_weight, Mask::Upper, &mut arms);
        let arms_weight: f32 = arms.iter().map(|layer| layer.weight).sum();
        if arms_weight <= 1e-3 {
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
        }
    }

    /// Starts and steps a great weapon's draw or sheathe (#1073): when a great weapon's holder
    /// changes its weapon mode, the upper body takes the weapon from or puts it across the back.
    /// A strike, a cast, death or reduced motion cuts it short, and the weapon moves at once.
    fn step_readying<D: Fn(&str) -> f32>(&mut self, frame: &Frame<'_, D>) {
        let cues = frame.cues;
        let before = std::mem::replace(&mut self.shown_drawn, cues.weapon_drawn);
        if let Some((clip, _, time)) = &mut self.readying {
            *time += frame.dt;
            if *time >= (frame.duration)(clip)
                || self.strike.is_some()
                || self.cast.is_some()
                || cues.dead
                || cues.reduced
            {
                self.readying = None;
            }
        }
        let great = cues.loadout.and_then(Loadout::great_weapon).is_some();
        if let (true, Some(before), Some(drawn)) = (great, before, cues.weapon_drawn)
            && before != drawn
            && !cues.dead
            && !cues.reduced
            && self.strike.is_none()
            && self.cast.is_none()
        {
            let clip = if drawn {
                GREAT_WEAPON_DRAW
            } else {
                GREAT_WEAPON_SHEATHE
            };
            self.readying = frame.available(clip).then(|| {
                self.readies = self.readies.wrapping_add(1);
                (clip, drawn, 0.)
            });
        }
    }

    /// Where the weapons, the throwable and a chore's item ride this frame.
    fn step_carry<D: Fn(&str) -> f32>(&self, frame: &Frame<'_, D>, chore: Option<Chore>) -> Carry {
        let cues = frame.cues;
        // A throw or a bandage moves the daggers (#603): a sheathed dagger's hand opens, a
        // drawn throwable's closes round it.
        let mut carry = self.carry(frame.clips, cues, frame.duration);
        // Never let sheathing retime a previously accepted strike/cast. Clips, not the next
        // input mode, own that action until completion (#802).
        carry.weapon_drawn =
            if cues.weapon_drawn == Some(false) && (self.strike.is_some() || self.cast.is_some()) {
                Some(true)
            } else {
                cues.weapon_drawn
            };
        // Through a great weapon's draw or sheathe it stays where it was until the clip's swap.
        if let Some((clip, drawn, time)) = self.readying {
            #[allow(clippy::cast_possible_truncation)]
            let swap = cues
                .motions
                .and_then(|motions| motions.get(clip))
                .and_then(|motion| motion.swap)
                .map_or((frame.duration)(clip) / 2., |swap| swap as f32);
            carry.weapon_drawn = Some(if time < swap { !drawn } else { drawn });
        }
        if carry.weapon_drawn == Some(false) {
            carry.sheathed = [true; 2];
        }
        carry.chore = chore;
        // An item in the weapon hand, the signal horn (#952), sends the weapon to its stowed socket.
        if carry
            .chore
            .is_some_and(|chore| chore.socket == "hand_r.weapon")
        {
            carry.weapon_drawn = Some(false);
        }
        carry
    }

    /// Steps each hand's grip over its fingers and appends its layers: a held item's grip
    /// closes that hand's fingers over every body clip (asset-tooling #285), but not while the
    /// hands hold a face, a ladder, a rope or a ledge (#500). `upper` is the upper body's clip.
    fn step_grips<D: Fn(&str) -> f32>(
        &mut self,
        frame: &Frame<'_, D>,
        carry: &Carry,
        upper: &'static str,
        first: bool,
        layers: &mut Vec<Layer>,
    ) {
        let Frame {
            clips,
            duration,
            cues,
            blend,
            dt,
            ..
        } = *frame;
        let climbing = self.held.holds() || self.mantle.is_some() || self.letting_go.is_some();
        let mut hands = cues.loadout.map(Loadout::hands).unwrap_or_default();
        // A chore's hand item closes the fingers round its handle or shaft.
        if let Some((hand, held)) = carry.chore.and_then(Chore::grip) {
            match hand {
                Hand::Left => hands.left.get_or_insert(held),
                Hand::Right => hands.right.get_or_insert(held),
            };
        }
        // A reload winds the crossbow's crank with the left hand, then hands the crossbow to the
        // left hand while the right draws a bolt (#607).
        let crossbow = carry.crossbow.unwrap_or_default();
        // The caster clips key the left hand's fingers (#609): no left-hand grip over them,
        // except a shield's, which stays in hand, and the staff's second hand over a staff
        // swing. The tome rides the back of the hand, so it needs none.
        let casting = clips.caster.filter(|caster| caster.contains(upper));
        let staff_swing = casting.is_some_and(|caster| caster.staff.contains(&upper));
        let left_open = casting.is_some() && hands.left != Some(Held::Shield);
        for hand in Hand::BOTH {
            let throwable = carry.throwable.filter(|_| hand == Hand::Right);
            let grip = throwable
                .map(Throwable::grip)
                .or_else(|| (crossbow.crank && hand == Hand::Left).then_some(CRANK_GRIP))
                .or_else(|| {
                    (staff_swing && hand == Hand::Left).then_some(Held::Staff.grip(Hand::Left))
                })
                .or_else(|| {
                    hands
                        .get(hand)
                        .filter(|_| {
                            carry.weapon_drawn != Some(false)
                                && !carry.sheathed[hand.index()]
                                && !(crossbow.left_hand && hand == Hand::Right)
                                && !(left_open && hand == Hand::Left)
                        })
                        .map(|held| held.grip(hand))
                })
                .filter(|&clip| !climbing && frame.available(clip))
                .map(|clip| Want::looped(clip, 0.));
            let track = &mut self.grips[hand.index()];
            // A unit's first frame grips at once, like its first clip.
            let seconds = if first { 0. } else { blend.upper_seconds };
            track.advance(grip, dt, seconds, cues.reduced);
            track.layers(duration, 1., Mask::Fingers(hand), layers);
        }
    }

    /// Steps the pose a unit holds (#619) and says which clip to show now: `wanted` asks for a
    /// pose, `blocked` says the unit cannot hold one (it moved, was struck or left the ground).
    fn step_posture(
        &mut self,
        social: Option<SocialClips>,
        wanted: Option<Posture>,
        blocked: bool,
        moving: bool,
        duration: &impl Fn(&str) -> f32,
        (now, reduced): (f64, bool),
    ) -> Option<Want> {
        let social = social?;
        let available = |clip: &&'static str| duration(clip) > 0.;
        if wanted.is_none() {
            self.posture_broken = false;
        } else if blocked && self.posture.is_some() {
            // Walking up to the corpse is not breaking a kneel; breaking one that shows is.
            self.posture_broken = true;
        }
        let wanted = wanted.filter(|&posture| {
            !self.posture_broken && !blocked && available(&social.chain(posture).1)
        });
        // Over and out: a finished exit, or moving input once the unit has stood up.
        if let Some(step) = self.posture
            && step.phase == PosturePhase::Exit
        {
            let (_, _, exit) = social.chain(step.posture);
            #[allow(clippy::cast_possible_truncation)]
            let elapsed = (now - step.since) as f32;
            if exit.is_none_or(|exit| elapsed >= duration(exit))
                || moving && elapsed >= step.posture.stand_seconds()
            {
                self.posture = None;
            }
        }
        match (self.posture, wanted) {
            (
                None
                | Some(PostureStep {
                    phase: PosturePhase::Exit,
                    ..
                }),
                Some(posture),
            ) => {
                let enter = social
                    .chain(posture)
                    .0
                    .filter(|_| !reduced)
                    .filter(available);
                self.postures = self.postures.wrapping_add(1);
                self.posture = Some(PostureStep {
                    posture,
                    phase: if enter.is_some() {
                        PosturePhase::Enter
                    } else {
                        PosturePhase::Loop
                    },
                    since: now,
                });
            }
            (Some(step), Some(posture)) if step.posture != posture => {
                // From one pose straight into the next: its enter, when it has one.
                let enter = social
                    .chain(posture)
                    .0
                    .filter(|_| !reduced)
                    .filter(available);
                self.postures = self.postures.wrapping_add(1);
                self.posture = Some(PostureStep {
                    posture,
                    phase: if enter.is_some() {
                        PosturePhase::Enter
                    } else {
                        PosturePhase::Loop
                    },
                    since: now,
                });
            }
            (Some(step), None) if step.phase != PosturePhase::Exit => {
                // Leaving by its own exit when it stands still and unbroken.
                let exit = social
                    .chain(step.posture)
                    .2
                    .filter(|_| !reduced && !moving && !self.posture_broken && !blocked);
                self.posture = exit.filter(available).map(|_| PostureStep {
                    phase: PosturePhase::Exit,
                    since: now,
                    ..step
                });
            }
            _ => {}
        }
        // The enter runs into the loop, on the presentation clock so reduced motion is frozen.
        if let Some(step) = &mut self.posture
            && step.phase == PosturePhase::Enter
            && {
                let (enter, looped, _) = social.chain(step.posture);
                // A one-way pose (the rise) holds its end instead of looping back.
                enter != Some(looped)
                    && enter.is_none_or(|enter| now - step.since >= f64::from(duration(enter)))
            }
        {
            step.phase = PosturePhase::Loop;
            step.since = now;
        }
        let step = self.posture?;
        let (enter, looped, exit) = social.chain(step.posture);
        #[allow(clippy::cast_possible_truncation)]
        let at = (now - step.since).max(0.) as f32;
        Some(match step.phase {
            PosturePhase::Enter => Want {
                at: Some(at),
                ..Want::hold(enter?, self.postures)
            },
            PosturePhase::Loop => Want::looped(looped, 1.),
            PosturePhase::Exit => Want {
                at: Some(at),
                ..Want::hold(exit?, self.postures)
            },
        })
    }

    /// Where a dagger holder's daggers ride and what it throws (#603), from the upper body's
    /// clip: a throw of a drawn item shows it on the belt until the clip's `drawSeconds` (#611),
    /// then sheathes the right dagger and holds the item until its `releaseSeconds`; a bandage
    /// sheathes both. Without the clip's timing the daggers stay in hand.
    fn carry(&self, clips: Clips, cues: Cues<'_>, duration: &impl Fn(&str) -> f32) -> Carry {
        if cues.loadout == Some(Loadout::Crossbow) {
            return Carry {
                crossbow: Some(self.crossbow(clips, cues, duration)),
                ..Carry::default()
            };
        }
        let Some(dagger) = clips
            .dagger
            .filter(|_| cues.loadout.is_some_and(Loadout::daggers) && !cues.dead)
        else {
            return Carry::default();
        };
        let Some((clip, time)) = self.upper.target(duration) else {
            return Carry::default();
        };
        if clip == dagger.bandage {
            return Carry {
                sheathed: [true; 2],
                ..Carry::default()
            };
        }
        let item = self
            .strike
            .or(self.cast)
            .filter(|strike| strike.clip == clip)
            .map(|strike| strike.item)
            .or_else(|| {
                self.preparation
                    .filter(|preparation| preparation.clip == clip)
                    .map(|preparation| preparation.item)
            })
            .flatten();
        let timing = cues
            .motions
            .and_then(|motions| motions.get(clip))
            .and_then(|motion| motion.draw.zip(motion.release));
        match (item, timing) {
            (Some(item), Some((draw, release))) if f64::from(time) >= draw => Carry {
                sheathed: [false, true],
                throwable: (f64::from(time) < release).then_some(item),
                ..Carry::default()
            },
            // Until the draw the item waits on the belt (#611).
            (Some(item), Some(_)) => Carry {
                belted: Some(item),
                ..Carry::default()
            },
            _ => Carry::default(),
        }
    }

    /// Where a crossbowman's crossbow and bolt ride (#607), from the upper body's clip and the
    /// clips' `crossbow` marks: the shot looses the bolt at its release; the reload winds the
    /// crank over `crankSeconds`, holds the crossbow in the left hand over `leftHandSeconds`,
    /// draws the next bolt at `boltDrawnSeconds` and lays it in the groove at
    /// `reloadedSeconds`. Otherwise the crossbow is loaded in the right hand.
    fn crossbow(
        &self,
        clips: Clips,
        cues: Cues<'_>,
        duration: &impl Fn(&str) -> f32,
    ) -> CrossbowCarry {
        let loaded = CrossbowCarry {
            bolt: Some(Bolt::Groove),
            ..CrossbowCarry::default()
        };
        let (Some(crossbow), Some((clip, time))) = (clips.crossbow, self.upper.target(duration))
        else {
            return loaded;
        };
        let Some(marks) = cues
            .motions
            .and_then(|motions| motions.get(clip))
            .and_then(|motion| motion.crossbow)
        else {
            return loaded;
        };
        let time = f64::from(time);
        let within =
            |span: Option<[f64; 2]>| span.is_some_and(|[from, to]| (from..to).contains(&time));
        if clip == crossbow.fire {
            CrossbowCarry {
                bolt: marks
                    .release
                    .is_none_or(|release| time < release)
                    .then_some(Bolt::Groove),
                ..CrossbowCarry::default()
            }
        } else if clip == crossbow.reload {
            let bolt = match (marks.bolt_drawn, marks.reloaded) {
                (Some(drawn), _) if time < drawn => None,
                (_, Some(reloaded)) if time >= reloaded => Some(Bolt::Groove),
                (Some(_), _) => Some(Bolt::Hand),
                (None, _) => Some(Bolt::Groove),
            };
            CrossbowCarry {
                left_hand: within(marks.left_hand),
                crank: within(marks.crank),
                bolt,
            }
        } else {
            loaded
        }
    }
}

/// One frame's inputs to [`Playback::advance`]: the clip table, the clip lengths, the cues, the
/// clamped blend, the frame step in seconds and whether the zone's guard protects.
struct Frame<'a, D> {
    clips: Clips,
    duration: &'a D,
    cues: Cues<'a>,
    blend: Blend,
    dt: f32,
    guarding: bool,
}

impl<D: Fn(&str) -> f32> Frame<'_, D> {
    /// The model has `clip`.
    fn available(&self, clip: &str) -> bool {
        (self.duration)(clip) > 0.
    }

    /// The sword clip `pick` names, when the model has it.
    fn sword(&self, pick: fn(SwordClips) -> &'static str) -> Option<&'static str> {
        self.clips
            .sword
            .map(pick)
            .filter(|clip| self.available(clip))
    }

    /// The swim clip `pick` names, when the model has it.
    fn swim(&self, pick: fn(SwimClips) -> &'static str) -> Option<&'static str> {
        self.clips
            .swim
            .map(pick)
            .filter(|clip| self.available(clip))
    }
}

/// What a frame's first steps settled, for the clip choice after them.
#[derive(Clone, Copy)]
struct Footing {
    /// The charge loop still running (#582).
    charge_run: Option<&'static str>,
    /// The stun clip held (#582).
    stun: Option<&'static str>,
    /// In swimming depth (Atlas 6.6.41).
    swims: bool,
    /// The zone's climb or mantle, free once dead (#500).
    held: TraversalView,
    /// Holding a face or playing out a mantle.
    traversing: bool,
    /// A confirmed blow's hitstop holds the strike clocks (#900).
    stopped: bool,
    /// Still on the ground, out of the water and off the wall.
    standing: bool,
    moving: bool,
    /// The model layers its upper body over its legs.
    layered: bool,
}

/// The emote, interaction, held pose and talk idle a frame shows (#619).
struct Poses {
    /// The gesture, and whether it takes the whole body.
    gesture: Option<(bool, Want)>,
    posture: Option<Want>,
    /// A held pose or a whole-body gesture keeps the upper body's idle stances off.
    occupied: bool,
    talk: Option<Want>,
    /// The pose or gesture faces the cue's `face`.
    turned: bool,
}

/// The strike or cast shown, whether it takes the whole body, and the local wind-up under it.
struct Shown {
    strike: Option<(bool, Want)>,
    prepared: Option<Want>,
    /// The strike shown leaves the legs mid-swing.
    split: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use mmorpg_core::PlayerClass;

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
                guard: guarding.then_some(StrikeHeight::Middle),
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
                .filter(|layer| {
                    layer.mask.drives(
                        Joint {
                            upper,
                            fingers: None,
                        },
                        [false; 2],
                    )
                })
                .map(|layer| layer.weight)
                .sum()
        };
        (sum(true), sum(false))
    }

    /// Each gait plays at the rate that keeps its planted foot still at the travel speed (#804),
    /// the speed over the clip's authored ground speed, as far as a natural cadence allows: a
    /// stride too short for the speed slides rather than doubling the legs' pace. The figures
    /// are the male Ranger's `humanoid-clips` values.
    #[test]
    fn gaits_keep_their_planted_foot_at_the_travel_speed() {
        let motions = crate::units::traversal::Motions::parse(
            r#"{"schema": "humanoid-clips-v1", "clips": [
                {"name": "Walk_Loop", "groundSpeedMetersPerSecond": 1.0434},
                {"name": "Jog_Fwd_Loop", "groundSpeedMetersPerSecond": 1.7637},
                {"name": "Run_Fwd_Loop", "groundSpeedMetersPerSecond": 6.3},
                {"name": "Sprint_Loop", "groundSpeedMetersPerSecond": 2.1345},
                {"name": "Jog_Bwd_Loop", "groundSpeedMetersPerSecond": 1.7637},
                {"name": "Walk_Bwd_Loop", "groundSpeedMetersPerSecond": 1.0434},
                {"name": "Run_Strafe_Left_Loop", "groundSpeedMetersPerSecond": 6.3},
                {"name": "Run_Strafe_Right_Loop", "groundSpeedMetersPerSecond": 6.3},
                {"name": "Walk_Strafe_Left_Loop", "sourceRootSpeedMetersPerSecond": 0.875},
                {"name": "Walk_Strafe_Right_Loop", "sourceRootSpeedMetersPerSecond": 0.875}
            ]}"#,
        )
        .expect("valid descriptor");
        let one = |_: &str| true;
        let gait = |motions, speed, direction| {
            Playback::default().gait(
                Kind::Ranger.clips(),
                &one,
                motions,
                (speed, direction),
                None,
            )
        };
        let run = f64::from(mmorpg_core::RUN_SPEED_UNITS_PER_SECOND) / 100.;
        let backpedal = f64::from(mmorpg_core::BACKPEDAL_SPEED_UNITS_PER_SECOND) / 100.;
        let creature_walk = 2.4;
        let pi = std::f64::consts::PI;
        for (speed, direction, clip, ground) in [
            (run, 0., "Run_Fwd_Loop", 6.3),
            (run, FRAC_PI_2, "Run_Strafe_Left_Loop", 6.3),
            (run, -FRAC_PI_2, "Run_Strafe_Right_Loop", 6.3),
            (1.2, 0., "Walk_Loop", 1.0434),
            (1.2, pi, "Walk_Bwd_Loop", 1.0434),
            (1.2, FRAC_PI_2, "Walk_Strafe_Left_Loop", 0.875),
        ] {
            let want = gait(Some(&motions), speed, direction);
            assert_eq!(want.clip, clip);
            // The foot's authored speed times the rate is the body's.
            let foot = f64::from(want.rate) * ground;
            assert!(
                (foot - speed).abs() < 1e-4,
                "{clip} at {speed} m/s: {foot} m/s"
            );
        }
        // Too short a stride for the speed keeps a natural cadence and slides instead: the walk
        // under a creature's 2.4 m/s, the jog backpedalling, the sprint charging.
        for (speed, direction, clip) in [
            (creature_walk, 0., "Walk_Loop"),
            (backpedal, pi, "Jog_Bwd_Loop"),
            (SPRINT_CLIP_SPEED + 1., 0., "Sprint_Loop"),
        ] {
            let want = gait(Some(&motions), speed, direction);
            assert_eq!(want.clip, clip);
            assert!((f64::from(want.rate) - GAIT_RATES[1]).abs() < 1e-6);
        }
        // A creeping walk and a glitched speed stay within the rate bounds.
        assert!((gait(Some(&motions), 0.3, 0.).rate - 0.5).abs() < 1e-6);
        assert!((gait(Some(&motions), 40., FRAC_PI_2).rate - 1.5).abs() < 1e-6);
        // A model whose clips record no ground speed (the animals) keeps the earlier pacing.
        assert!((gait(None, run, 0.).rate - 1.26).abs() < 1e-4);
        assert!((gait(None, creature_walk, 0.).rate - 2.4 / 1.8).abs() < 1e-4);
    }

    /// The fetched Ranger bundles' own gait metadata (#804): at every speed the game moves a
    /// figure, its legs keep a natural cadence, and the player's run and side-runs plant their
    /// feet at about their authored pace. #919 played the jog at 3.6 times and the walk under a
    /// creature at 2.3 times, legs visibly spinning at double speed.
    #[test]
    fn the_fetched_rangers_walk_and_run_at_a_natural_cadence() {
        let run = f64::from(mmorpg_core::RUN_SPEED_UNITS_PER_SECOND) / 100.;
        let backpedal = f64::from(mmorpg_core::BACKPEDAL_SPEED_UNITS_PER_SECOND) / 100.;
        let creature_walk = 2.4;
        let pi = std::f64::consts::PI;
        for sex in ["male", "female"] {
            let Some(json) = crate::fetched_bundles::read(&format!(
                "characters/humanoid.ranger.{sex}/humanoid.ranger.{sex}.default.humanoid-clips.json"
            )) else {
                continue;
            };
            let motions = Motions::parse(&json).expect("valid descriptor");
            // Every gait records its ground speed, so a gait the descriptor lacks is missing.
            let available = |clip: &str| motions.get(clip).is_some();
            for (speed, direction, running) in [
                (run, 0., true),
                (run, FRAC_PI_2, true),
                (run, -FRAC_PI_2, true),
                (backpedal, pi, false),
                (creature_walk, 0., false),
                (run / 2., 0., false),
                (run / 2., FRAC_PI_2, false),
                (1.2, pi, false),
            ] {
                let want = Playback::default().gait(
                    Kind::Ranger.clips(),
                    &available,
                    Some(&motions),
                    (speed, direction),
                    None,
                );
                assert!(
                    (0.5..=1.5).contains(&want.rate),
                    "{sex} {} at {speed} m/s plays at {}",
                    want.clip,
                    want.rate
                );
                if running {
                    assert!(want.clip.starts_with("Run_"), "{sex} runs {}", want.clip);
                    assert!((want.rate - 1.).abs() < 0.05, "{sex} {}", want.rate);
                }
            }
        }
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
        assert_eq!(gait(run, 0., true), "Run_Fwd_Loop");
        // A diagonal stays forward.
        assert_eq!(gait(run, pi / 4., true), "Run_Fwd_Loop");
        assert_eq!(gait(run, FRAC_PI_2, true), "Run_Strafe_Left_Loop");
        assert_eq!(gait(run, -FRAC_PI_2, true), "Run_Strafe_Right_Loop");
        assert_eq!(gait(run, pi, true), "Jog_Bwd_Loop");
        assert_eq!(gait(backpedal, -2.4, true), "Jog_Bwd_Loop");
        assert_eq!(gait(run / 2., pi, true), "Walk_Bwd_Loop");
        assert_eq!(gait(run / 2., FRAC_PI_2, true), "Walk_Strafe_Left_Loop");
        assert_eq!(gait(run / 2., 0., true), "Walk_Loop");
        assert_eq!(gait(SPRINT_CLIP_SPEED + 1., 0., true), "Sprint_Loop");
        // Without the directional clips: the run for strafes, the walk for backpedalling.
        assert_eq!(gait(run, FRAC_PI_2, false), "Run_Fwd_Loop");
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
    fn held_items_grip_their_hands_fingers_except_while_climbing() {
        let clips = Kind::Ranger.clips();
        let frame = |playback: &mut Playback, loadout: Loadout, traversal, now: f64| {
            playback.advance(
                clips,
                |_| 1.,
                Cues {
                    traversal,
                    loadout: Some(loadout),
                    now,
                    dt: 0.05,
                    ..Cues::default()
                },
                Blend::default(),
            )
        };
        let grips = |animation: &Animation| -> Vec<_> {
            animation
                .layers
                .iter()
                .filter(|layer| matches!(layer.mask, Mask::Fingers(_)))
                .map(|layer| (layer.clip, layer.mask, layer.weight))
                .collect()
        };
        let mut playback = Playback::default();
        let free = TraversalView::Free;
        let warden = frame(&mut playback, Loadout::SwordAndShield, free, 0.05);
        assert_eq!(
            grips(&warden),
            [
                ("Grip_Shield_L", Mask::Fingers(Hand::Left), 1.),
                ("Grip_Weapon_R", Mask::Fingers(Hand::Right), 1.),
            ]
        );
        // Climbing lets go of the grips; they come back once the climb ends.
        let climbing = TraversalView::Climbing;
        let mut run = |traversal, from: u32, to: u32| {
            (from..=to)
                .map(|step| {
                    let now = f64::from(step) * 0.05;
                    frame(&mut playback, Loadout::SwordAndShield, traversal, now)
                })
                .last()
                .expect("frames")
        };
        assert!(grips(&run(climbing, 2, 6)).is_empty());
        // The let-go plays out (1 s here) before the hands close again.
        assert!(grips(&run(free, 7, 20)).is_empty());
        assert_eq!(grips(&run(free, 21, 30)).len(), 2);
        // The Arcanist holds the tome and the staff; the Ranger's hands are empty.
        let arcanist = frame(
            &mut Playback::default(),
            Loadout::CrystalStaffAndTome,
            free,
            0.,
        );
        let held: Vec<_> = grips(&arcanist)
            .into_iter()
            .map(|(clip, ..)| clip)
            .collect();
        assert_eq!(held, ["Grip_Book_L", "Grip_Staff_R"]);
        let ranger = frame(&mut Playback::default(), Loadout::BowAndQuiver, free, 0.);
        assert!(grips(&ranger).is_empty());
        // A grip owns its hand's fingers over the body clips, which still drive the other hand.
        let joint = |name: &str| Joint::new(name, true);
        let gripped = gripped(&warden.layers);
        assert_eq!(gripped, [true, true]);
        let only_right = [false, true];
        assert!(!Mask::All.drives(joint("index_01_r"), only_right));
        assert!(Mask::Upper.drives(joint("index_01_l"), only_right));
        assert!(Mask::Upper.drives(joint("hand_r"), only_right));
        assert!(Mask::Fingers(Hand::Right).drives(joint("thumb_03_r"), only_right));
        assert!(!Mask::Fingers(Hand::Right).drives(joint("thumb_03_l"), only_right));
        assert!(!Mask::Fingers(Hand::Right).drives(joint("hand_r"), only_right));
    }

    /// A lynx's pounce (#989) plays its wind-up, its lunge and its recovery in turn.
    #[test]
    fn a_pounce_plays_its_wind_up_lunge_and_recovery() {
        let clips = Kind::Lynx.clips();
        let frame = |playback: &mut Playback, now: f64| {
            playback.advance(
                clips,
                |_| 1.,
                Cues {
                    now,
                    dt: 0.05,
                    ..Cues::default()
                },
                Blend::default(),
            )
        };
        let mut playback = Playback::default();
        playback.strike("Pounce_Windup", 0.5, Region::Whole, 0.);
        playback.recover_through(("Pounce_Lunge", 0.5), ("Pounce_Recover", 0.83));
        assert_eq!(frame(&mut playback, 0.2).lower.0, "Pounce_Windup");
        assert_eq!(frame(&mut playback, 0.7).lower.0, "Pounce_Lunge");
        assert_eq!(frame(&mut playback, 1.4).lower.0, "Pounce_Recover");
        assert_eq!(frame(&mut playback, 2.2).lower.0, "Idle");
    }

    /// An adder's strike (#990) hisses through its warning, strikes and recovers, then coils.
    #[test]
    fn an_adder_strike_hisses_strikes_and_recovers() {
        let clips = Kind::Adder.clips();
        let frame = |playback: &mut Playback, now: f64| {
            playback.advance(
                clips,
                |_| 1.,
                Cues {
                    now,
                    dt: 0.05,
                    ..Cues::default()
                },
                Blend::default(),
            )
        };
        let mut playback = Playback::default();
        playback.strike("Warn_Hiss_Loop", 1.333, Region::Whole, 0.);
        playback.recover_through(("Strike", 0.417), ("Strike_Recover", 0.833));
        assert_eq!(frame(&mut playback, 0.6).lower.0, "Warn_Hiss_Loop");
        assert_eq!(frame(&mut playback, 1.5).lower.0, "Strike");
        assert_eq!(frame(&mut playback, 2.0).lower.0, "Strike_Recover");
        assert_eq!(frame(&mut playback, 3.0).lower.0, "Idle_Coil_Loop");
    }

    /// A bat (#995) hangs in its perch out of a fight, hovers in one, and dives through its
    /// wind-up, contact and recovery back to its hover.
    #[test]
    fn a_bat_perches_hovers_and_dives() {
        let clips = Kind::Bat.clips();
        let frame = |playback: &mut Playback, now: f64, engaged: bool| {
            playback.advance(
                clips,
                |_| 1.,
                Cues {
                    now,
                    dt: 0.05,
                    engaged,
                    ..Cues::default()
                },
                Blend::default(),
            )
        };
        let mut playback = Playback::default();
        assert_eq!(frame(&mut playback, 0., false).lower.0, "Perch");
        assert_eq!(frame(&mut playback, 0.1, true).lower.0, "Idle");
        playback.strike("Dive_Windup", 0.5, Region::Whole, 0.2);
        playback.recover_through(("Dive_Contact", 0.5), ("Dive_Recover", 0.833));
        assert_eq!(frame(&mut playback, 0.5, true).lower.0, "Dive_Windup");
        assert_eq!(frame(&mut playback, 0.9, true).lower.0, "Dive_Contact");
        assert_eq!(frame(&mut playback, 1.5, true).lower.0, "Dive_Recover");
        assert_eq!(frame(&mut playback, 2.5, true).lower.0, "Idle");
        assert!(Kind::Bat.corpse_drop() > 0. && Kind::Adder.corpse_drop() == 0.);
    }

    /// A stag (#997) grazes out of a fight, stands alert through its charge's warning, charges
    /// in `Attack_A` (its contact the zone's, 1.625 s in) and stands in a fight once it is done.
    #[test]
    fn a_stag_grazes_alerts_and_charges() {
        let clips = Kind::Stag.clips();
        let frame = |playback: &mut Playback, now: f64, engaged: bool| {
            playback.advance(
                clips,
                |_| 1.,
                Cues {
                    now,
                    dt: 0.05,
                    engaged,
                    ..Cues::default()
                },
                Blend::default(),
            )
        };
        let mut playback = Playback::default();
        assert_eq!(frame(&mut playback, 0., false).lower.0, "Eat");
        playback.strike("Alert", 1., Region::Whole, 0.1);
        playback.recover_through(("Attack_A", 1.333), ("", 0.));
        assert_eq!(frame(&mut playback, 0.6, true).lower.0, "Alert");
        assert_eq!(frame(&mut playback, 1.5, true).lower.0, "Attack_A");
        assert_eq!(frame(&mut playback, 2.3, true).lower.0, "Attack_A");
        assert_eq!(frame(&mut playback, 2.6, true).lower.0, "Idle");
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
        playback.strike("Sword_Attack", 1.5, Region::Footwork, 0.1);
        playback.react(Reaction::Chest, "Hit_Chest", 0.33, 0.2);
        assert_eq!(frame(&mut playback, 0., 0.25).upper.0, "Sword_Attack");
        // A spell cast raises, then holds on the upper body until it resolves.
        let mut playback = Playback::default();
        playback.cast(
            Some(("Spell_Simple_Enter", 0.5)),
            ("Spell_Simple_Idle_Loop", true),
            Region::Upper,
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
        playback.cancel(Some(("Spell_Simple_Exit", 0.4)), false, 1.);
        assert_eq!(frame(&mut playback, 0., 1.1).upper.0, "Spell_Simple_Exit");
        // The bow draws and holds through a cast, then releases and recovers, whole body.
        let mut playback = Playback::default();
        playback.cast(
            Some(("Bow_Draw", 0.7)),
            ("Bow_Hold_Loop", true),
            Region::Footwork,
            2.,
            0.,
        );
        assert_eq!(frame(&mut playback, 0., 0.1).lower.0, "Bow_Draw");
        assert_eq!(frame(&mut playback, 0., 1.).lower.0, "Bow_Hold_Loop");
        playback.strike("Bow_Release", 0.47, Region::Footwork, 2.);
        playback.recover("Bow_Recovery", 1.1);
        assert_eq!(frame(&mut playback, 0., 2.1).lower.0, "Bow_Release");
        assert_eq!(frame(&mut playback, 0., 2.8).lower.0, "Bow_Recovery");
        assert_eq!(frame(&mut playback, 0., 3.8).lower.0, "Idle_Loop");
        // A model without casts has none to start; a beast flinches with its one hit (#889).
        assert!(Kind::Wolf.cast(Some(PlayerClass::Ranger), None).is_none());
        assert_eq!(Kind::Wolf.reaction(Reaction::Chest), Some("Hit"));
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
            "Shield_Guard_Mid_Loop"
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
        assert_eq!(walking.upper.0, "Shield_Guard_Mid_Loop");
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
            "Run_Fwd_Loop"
        );
        playback.strike("Sword_Attack", 1., Region::Footwork, 0.);
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
        assert_eq!(running.lower.0, "Run_Fwd_Loop");
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
            ("Death_Back", 1.),
            "the first of the varied deaths (#885) the bundle has"
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
            "Run_Fwd_Loop"
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
            "Death_Back"
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
        playback.prepare("Sword_Attack", WINDUP_HOLD_SECONDS, None, now);
        for _ in 0..20 {
            frame(&mut playback, &mut now);
        }
        // An unanswered wind-up holds short of the swing, on the arms only.
        let held = frame(&mut playback, &mut now);
        assert_eq!(held.upper, ("Sword_Attack", WINDUP_HOLD_SECONDS));
        assert_eq!(held.lower.0, "Idle_Loop");
        // The accepted strike continues from the hold instead of restarting.
        playback.commit("Sword_Attack", 1.5, Region::Footwork, now);
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
        refused.prepare("Sword_Attack", WINDUP_HOLD_SECONDS, None, now);
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
        playback.strike("Sword_Attack", 1.5, Region::Footwork, now);
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
        assert_eq!(guard.upper.0, "Shield_Guard_Mid_Loop");
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
        // The legs keep the shield stance; the arms hold the middle lane's shield (#582).
        assert!(frame(&mut playback, &mut now, 0., true).layers.iter().all(
            |layer| match layer.mask {
                Mask::Upper => layer.clip == "Shield_Guard_Mid_Loop",
                _ => layer.clip == "Idle_Shield_Loop",
            }
        ));
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
        playback.strike("Sword_Attack", 1., Region::Footwork, 0.);
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
        playback.strike("Sword_Attack", 1., Region::Footwork, 0.1);
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
    fn the_sword_clips_play_guard_lanes_stuns_rises_charges_and_held_wind_ups() {
        let clips = Kind::Ranger.clips();
        let with = |clip: &str| if clip == "Sword_Whirl" { 1.2 } else { 0.6 };
        // A bundle from before asset-tooling #447: none of the sword clips.
        let without = |clip: &str| {
            if clip.starts_with("Sword_Slash")
                || clip.starts_with("Shield_Guard")
                || clip.starts_with("Charge_")
                || ["Stun_Loop", "Respawn_Rise", "Sword_Whirl"].contains(&clip)
            {
                0.
            } else {
                0.6
            }
        };
        let frame = |playback: &mut Playback,
                     duration: &dyn Fn(&str) -> f32,
                     now: f64,
                     cues: Cues<'static>| {
            playback.advance(
                clips,
                duration,
                Cues {
                    now,
                    dt: 0.05,
                    ..cues
                },
                Blend::default(),
            )
        };
        let guard = |lane| Cues {
            guard: Some(lane),
            ..Cues::default()
        };
        // Guard lanes raise the shield on the upper body; without them the shield idle.
        let high = frame(
            &mut Playback::default(),
            &with,
            0.,
            guard(StrikeHeight::High),
        );
        assert_eq!(high.upper.0, "Shield_Guard_High_Loop");
        let old = frame(
            &mut Playback::default(),
            &without,
            0.,
            guard(StrikeHeight::Low),
        );
        assert_eq!(old.upper.0, "Idle_Shield_Loop");
        // A stun reels the whole body; without the clip the idle stays.
        let stunned = Cues {
            stunned: true,
            ..Cues::default()
        };
        let reel = frame(&mut Playback::default(), &with, 0., stunned);
        assert_eq!((reel.lower.0, reel.upper.0), ("Stun_Loop", "Stun_Loop"));
        assert_eq!(
            frame(&mut Playback::default(), &without, 0., stunned)
                .lower
                .0,
            "Idle_Loop"
        );
        // A root holds the legs still and the upper body strains against vines or ice (#956).
        for (hold, clip) in [
            (
                crate::units::effects::Condition::Roots,
                "Rooted_Struggle_Loop",
            ),
            (
                crate::units::effects::Condition::Frost,
                "Rooted_Struggle_Frozen_Loop",
            ),
        ] {
            let rooted = Cues {
                rooted: Some(hold),
                ..Cues::default()
            };
            let held = frame(&mut Playback::default(), &with, 0., rooted);
            assert_eq!((held.lower.0, held.upper.0), ("Idle_Loop", clip));
        }
        // Back from death the unit rises once.
        let mut playback = Playback::default();
        let dead = Cues {
            dead: true,
            ..Cues::default()
        };
        frame(&mut playback, &with, 0., dead);
        assert_eq!(
            frame(&mut playback, &with, 0.1, Cues::default()).lower.0,
            "Respawn_Rise"
        );
        assert_eq!(
            frame(&mut playback, &with, 1., Cues::default()).lower.0,
            "Idle_Loop"
        );
        // A charge rushes while carried and strikes with the shield once it slows.
        let mut playback = Playback::default();
        playback.charge(0.);
        let rushing = Cues {
            speed: 13.5,
            ..Cues::default()
        };
        assert_eq!(
            frame(&mut playback, &with, 0.1, rushing).lower.0,
            "Charge_Run_Loop"
        );
        assert_eq!(
            frame(&mut playback, &with, 0.5, rushing).lower.0,
            "Charge_Run_Loop"
        );
        assert!(playback.charging());
        let arrived = frame(&mut playback, &with, 0.6, Cues::default());
        assert_eq!(arrived.lower.0, "Charge_Impact");
        assert!(!playback.charging());
        // The whirl winds up through its cast, holds, and swings on from the hold.
        let mut playback = Playback::default();
        playback.wind_up("Sword_Whirl", 1.2, 0.39, Region::Whole, 1.5, 0.);
        assert_eq!(
            frame(&mut playback, &with, 1., Cues::default()).lower,
            ("Sword_Whirl", 0.39)
        );
        playback.strike("Sword_Whirl", 1.2, Region::Whole, 1.5);
        let swing = frame(&mut playback, &with, 1.6, Cues::default());
        assert_eq!(swing.lower.0, "Sword_Whirl");
        assert!((swing.lower.1 - 0.49).abs() < 1e-4, "{swing:?}");
        // A slash drives only the upper body, even standing.
        let mut playback = Playback::default();
        playback.strike("Sword_Slash_Mid_L", 0.6, Region::Upper, 0.);
        let slash = frame(&mut playback, &with, 0.1, Cues::default());
        assert_eq!(
            (slash.lower.0, slash.upper.0),
            ("Idle_Loop", "Sword_Slash_Mid_L")
        );
        // A directed slash winds up to its peak; a contact time sets any other strike's hold.
        assert!((windup_seconds(0.6, Some(0.35), true) - SLASH_WINDUP_SECONDS).abs() < 1e-6);
        assert!((windup_seconds(1.2, Some(0.65), false) - 0.39).abs() < 1e-6);
        assert!((windup_seconds(1.5, None, false) - WINDUP_HOLD_SECONDS).abs() < 1e-6);
        // A timed swing (#679) holds its peak through the zone's wind-up, then swings on.
        let mut playback = Playback::default();
        playback.timed(
            "Sword_Slash_Mid_L",
            0.6,
            SLASH_WINDUP_SECONDS,
            Region::Upper,
            [0.3, 0.15],
            0.,
        );
        let held = frame(&mut playback, &with, 0.25, Cues::default());
        assert!((held.upper.1 - SLASH_WINDUP_SECONDS * (0.25 / 0.3)).abs() < 1e-4);
        let swung = frame(&mut playback, &with, 0.4, Cues::default());
        assert!(
            (swung.upper.1
                - (SLASH_WINDUP_SECONDS + (0.6 * 0.68 - SLASH_WINDUP_SECONDS) * (0.1 / 0.15)))
                .abs()
                < 1e-4,
            "{swung:?}"
        );
    }

    #[test]
    fn a_confirmed_blow_holds_the_fitted_strike_for_its_hitstop() {
        use crate::units::contact::{HITSTOP_RATE, hitstop_seconds, weight};
        // A fitted wind-up (#900) meets the zone's contact exactly and plays on at authored
        // speed; a contact the bounded rate cannot reach in time starts into the clip.
        let fit = Fit::new(0.6, 0.383);
        assert!((fit.at(0.383) - 0.6).abs() < 1e-6);
        assert!((fit.at(0.5) - 0.717).abs() < 1e-6);
        assert!((Fit::new(0.4, 0.1).at(0.) - (0.4 - 0.1 * FIT_RATES[1])).abs() < 1e-6);
        let clips = Kind::Ranger.clips();
        let dt = 1. / 60.;
        let zone = 28. / 60.;
        let stop = hitstop_seconds(weight("Mace_Swing_C"));
        let swing = |hit: bool| {
            let mut playback = Playback::default();
            let melee = Melee {
                fit: Some(Fit::new(0.4667, zone)),
                ..Melee::default()
            };
            playback.melee("Mace_Swing_C", 0.9, Region::Footwork, melee, 0.);
            (1..=40)
                .map(|frame| {
                    let now = f64::from(frame) * f64::from(dt);
                    if hit && frame == 28 {
                        playback.hitstop(stop, now);
                    }
                    let cues = Cues {
                        now,
                        dt: f64::from(dt),
                        ..Cues::default()
                    };
                    playback
                        .advance(clips, |_| 0.9, cues, Blend::default())
                        .lower
                })
                .collect::<Vec<_>>()
        };
        let (clean, struck) = (swing(false), swing(true));
        // The contact frame shows on the zone's contact tick.
        assert_eq!(clean[27].0, "Mace_Swing_C");
        assert!((clean[27].1 - 0.4667).abs() < 1e-3, "{:?}", clean[27]);
        // Through the hitstop the swing all but stops …
        assert!(struck[30].1 - struck[28].1 < 2. * dt * HITSTOP_RATE + 1e-4);
        // … and after it plays on, behind by the held time, which the weapon's weight sets.
        let behind = clean[39].1 - struck[39].1;
        let held = stop * (1. - HITSTOP_RATE);
        assert!((behind - held).abs() <= dt, "{behind} behind, {held} held");
        assert!(stop > hitstop_seconds(weight("Dagger_Stab_R")));
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
        assert!((pitch - crate::units::swim::SURFACE_PITCH).abs() < 0.01);
        // Climbing out eases back upright rather than snapping.
        now += 0.1;
        let out = swim_frame(&mut playback, one, WaterMode::Dry, 2., 0., now);
        assert!(out.attitude.pitch > 0. && out.attitude.pitch < pitch);
        // A strike while treading stays on the upper body: no footwork without footing.
        playback.strike("Sword_Attack", 1., Region::Footwork, now);
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
        assert_eq!(after.lower.0, "Run_Fwd_Loop");
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
        let motions = crate::units::traversal::Motions::parse(
            r#"{"schema": "humanoid-clips-v1", "clips": [
                {"name": "Climb_Up_Loop", "climbSpeedMetersPerSecond": 0.45}
            ]}"#,
        )
        .expect("valid descriptor");
        let one = |_: &str| 1.;
        let clips = Kind::Ranger.clips();
        let cues = |motions| Cues {
            traversal: TraversalView::Climbing,
            motions,
            vertical_speed: 0.9,
            ..Cues::default()
        };
        let authored = Playback::climb(clips, &one, cues(Some(&motions)));
        assert_eq!(
            (authored.clip, authored.rate),
            ("Climb_Up_Loop", 1.6_f32.min(2.))
        );
        let fallback = Playback::climb(clips, &one, cues(None));
        assert!((fallback.rate - 1.).abs() < 1e-6);
        // A bundle without the climb clips keeps the earlier in-place walk.
        let missing = |clip: &str| if clip.starts_with("Climb_") { 0. } else { 1. };
        assert_eq!(
            Playback::climb(clips, &missing, cues(None)).clip,
            "Walk_Loop"
        );
    }

    /// On a ladder (#621) the loops' phase follows the feet up the rung grid: one loop per rung,
    /// starting where the clip's first plants meet rungs.
    #[test]
    fn ladder_loops_follow_the_rung_grid() {
        let half = |_: &str| 0.5;
        let clips = Kind::Ranger.clips();
        let cues = |offset, vertical_speed| Cues {
            traversal: TraversalView::ClimbingLadder,
            rungs: Some(offset),
            vertical_speed,
            ..Cues::default()
        };
        let up = Playback::climb(clips, &half, cues(LADDER_UP_RUNG_OFFSET, 0.9));
        assert_eq!((up.clip, up.at), ("Ladder_Up_Loop", Some(0.)));
        let up = Playback::climb(clips, &half, cues(LADDER_UP_RUNG_OFFSET + 0.015, 0.9));
        assert!((up.at.unwrap() - 0.025).abs() < 1e-4, "{:?}", up.at);
        let down = Playback::climb(clips, &half, cues(LADDER_DOWN_RUNG_OFFSET - 0.06, -0.9));
        assert_eq!(down.clip, "Ladder_Down_Loop");
        assert!((down.at.unwrap() - 0.1).abs() < 1e-4, "{:?}", down.at);
        // Holding still plays the hold; a face's loops keep their own clock.
        assert_eq!(Playback::climb(clips, &half, cues(0.1, 0.)).at, None);
        let face = Cues {
            traversal: TraversalView::Climbing,
            ..cues(0.1, 0.9)
        };
        assert_eq!(Playback::climb(clips, &half, face).at, None);
    }

    #[test]
    fn a_throw_sheathes_the_dagger_and_holds_its_item_from_draw_to_release() {
        let motions = crate::units::traversal::Motions::parse(
            r#"{"schema": "humanoid-clips-v1", "clips": [
                {"name": "Throw_Knife", "drawSeconds": 0.09, "releaseSeconds": 0.28,
                 "contact": {"seconds": 0.28}}
            ]}"#,
        )
        .expect("valid descriptor");
        let clips = Kind::Ranger.clips();
        let lengths = |clip: &str| match clip {
            "Throw_Knife" => 0.5,
            "Bandage_Self" => 1.5,
            _ => 1.,
        };
        let frame = |playback: &mut Playback, loadout, now: f64| {
            playback.advance(
                clips,
                lengths,
                Cues {
                    motions: Some(&motions),
                    loadout: Some(loadout),
                    now,
                    dt: 0.02,
                    ..Cues::default()
                },
                Blend::default(),
            )
        };
        let grip = |animation: &Animation, hand| {
            animation
                .layers
                .iter()
                .filter(|layer| layer.mask == Mask::Fingers(hand) && layer.weight > 0.5)
                .map(|layer| layer.clip)
                .next()
        };
        let mut playback = Playback::default();
        let stance = frame(&mut playback, Loadout::TwinDaggers, 0.);
        // Standing, both daggers ride in hand over the dagger stance.
        assert_eq!(stance.upper.0, "Dagger_Idle_Loop");
        assert_eq!(stance.carry, Carry::default());
        assert_eq!(grip(&stance, Hand::Left), Some("Grip_Weapon_L"));
        playback.strike("Throw_Knife", 0.5, Region::Upper, 0.02);
        playback.throwing(Throwable::Knife);
        let at = |playback: &mut Playback, time: f64| {
            let mut last = None;
            let mut now = 0.02;
            while now <= 0.02 + time + 1e-9 {
                last = Some(frame(playback, Loadout::TwinDaggers, now));
                now += 0.02;
            }
            last.expect("frames")
        };
        let reaching = at(&mut playback.clone(), 0.04);
        assert_eq!(reaching.upper.0, "Throw_Knife");
        // Reaching for it, the knife still hangs on the belt (#611).
        assert_eq!(
            reaching.carry,
            Carry {
                belted: Some(Throwable::Knife),
                ..Carry::default()
            }
        );
        let drawn = at(&mut playback.clone(), 0.2);
        assert_eq!(
            drawn.carry,
            Carry {
                sheathed: [false, true],
                throwable: Some(Throwable::Knife),
                ..Carry::default()
            }
        );
        assert_eq!(grip(&drawn, Hand::Right), Some("Grip_Knife_Pinch_R"));
        let released = at(&mut playback.clone(), 0.4);
        assert_eq!(released.carry.sheathed, [false, true]);
        assert_eq!(released.carry.throwable, None);
        // The dagger is back in hand once the clip ends.
        let after = at(&mut playback, 0.7);
        assert_eq!(after.carry, Carry::default());
        // A bandage sheathes both daggers; a sword stays in hand.
        for (loadout, sheathed) in [
            (Loadout::TwinDaggers, [true; 2]),
            (Loadout::Sword, [false; 2]),
        ] {
            let mut bandaging = Playback::default();
            bandaging.cast(None, ("Bandage_Self", true), Region::Upper, 3., 0.);
            let shown = frame(&mut bandaging, loadout, 0.5);
            assert_eq!(shown.upper.0, "Bandage_Self");
            assert_eq!(shown.carry.sheathed, sheathed);
        }
        // Without the throw clip's timing the dagger stays in hand.
        let mut untimed = Playback::default();
        untimed.strike("Throw_Lob", 0.8, Region::Upper, 0.);
        untimed.throwing(Throwable::Flask);
        let shown = untimed.advance(
            clips,
            lengths,
            Cues {
                loadout: Some(Loadout::TwinDaggers),
                now: 0.4,
                dt: 0.02,
                ..Cues::default()
            },
            Blend::default(),
        );
        assert_eq!(shown.carry, Carry::default());
    }

    #[test]
    fn a_bundle_without_the_dagger_clips_keeps_the_idle_arms() {
        let mut playback = Playback::default();
        let shown = playback.advance(
            Kind::Ranger.clips(),
            |clip: &str| if clip.starts_with("Dagger_") { 0. } else { 1. },
            Cues {
                loadout: Some(Loadout::TwinDaggers),
                dt: 0.02,
                ..Cues::default()
            },
            Blend::default(),
        );
        assert_eq!(shown.upper.0, "Idle_Loop");
    }

    #[test]
    fn a_bow_shot_on_the_upper_body_alone_plays_the_upper_body_cycle() {
        let clips = Kind::Ranger.clips();
        let frame = |playback: &mut Playback, lengths: fn(&str) -> f32, speed: f64, now: f64| {
            playback.advance(
                clips,
                lengths,
                Cues {
                    speed,
                    loadout: Some(Loadout::BowAndQuiver),
                    now,
                    dt: 0.05,
                    ..Cues::default()
                },
                Blend::default(),
            )
        };
        let every: fn(&str) -> f32 = |_| 1.;
        // Standing, the release keeps the full-body cycle, pelvis turn and all.
        let mut playback = Playback::default();
        playback.strike("Bow_Release", 0.47, Region::Footwork, 0.);
        playback.recover("Bow_Recovery", 1.1);
        let standing = frame(&mut playback, every, 0., 0.05);
        assert_eq!(
            (standing.lower.0, standing.upper.0),
            ("Bow_Release", "Bow_Release")
        );
        // Walking, the upper body swaps in the hips-square twin over the gait, in step, and
        // recovers with the twin too.
        let walking = frame(&mut playback, every, 1.5, 0.1);
        assert_eq!(walking.upper.0, "Bow_Release_Upper");
        assert!((walking.upper.1 - 0.1).abs() < 1e-4);
        assert_eq!(walking.lower.0, "Walk_Loop");
        assert_eq!(
            frame(&mut playback, every, 1.5, 0.8).upper.0,
            "Bow_Recovery_Upper"
        );
        assert_eq!(
            Kind::Ranger.bow_shot(&walking).map(|shot| shot.clip),
            Some("Bow_Release_Upper")
        );
        // A cast's draw and hold on the move, and the local aim even standing (it never moves
        // the legs).
        let mut casting = Playback::default();
        casting.cast(
            Some(("Bow_Draw", 0.7)),
            ("Bow_Hold_Loop", true),
            Region::Footwork,
            2.,
            0.,
        );
        assert_eq!(
            frame(&mut casting, every, 3., 0.1).upper.0,
            "Bow_Draw_Upper"
        );
        assert_eq!(
            frame(&mut casting, every, 3., 1.).upper.0,
            "Bow_Hold_Upper_Loop"
        );
        let mut aiming = Playback::default();
        aiming.prepare("Bow_Aim_Loop", 0.3, None, 0.);
        assert_eq!(
            frame(&mut aiming, every, 0., 0.1).upper.0,
            "Bow_Aim_Upper_Loop"
        );
        // A bundle without the twins keeps the full-body clip on the upper body.
        let old: fn(&str) -> f32 = |clip| if clip.contains("_Upper") { 0. } else { 1. };
        let mut playback = Playback::default();
        playback.strike("Bow_Release", 0.47, Region::Footwork, 0.);
        frame(&mut playback, old, 0., 0.05);
        assert_eq!(frame(&mut playback, old, 1.5, 0.1).upper.0, "Bow_Release");
    }

    #[test]
    fn a_crossbowman_aims_fires_and_reloads_handing_the_crossbow_to_the_left_hand() {
        let motions = crate::units::traversal::Motions::parse(
            r#"{"schema": "humanoid-clips-v1", "clips": [
                {"name": "Crossbow_Fire", "crossbow": {"releaseSeconds": 0.067},
                 "contact": {"seconds": 0.083}},
                {"name": "Crossbow_Reload", "crossbow": {"crankSeconds": [0.32, 0.85],
                 "spannedSeconds": 0.85, "leftHandSeconds": [0.96, 1.44],
                 "boltDrawnSeconds": 1.15, "reloadedSeconds": 1.34}}
            ]}"#,
        )
        .expect("valid descriptor");
        let clips = Kind::Ranger.clips();
        let lengths = |clip: &str| match clip {
            "Crossbow_Fire" => 0.4,
            "Crossbow_Reload" => 1.6,
            _ => 2.,
        };
        let frame = |playback: &mut Playback, speed: f64, engaged: bool, now: f64| {
            playback.advance(
                clips,
                lengths,
                Cues {
                    motions: Some(&motions),
                    speed,
                    engaged,
                    loadout: Some(Loadout::Crossbow),
                    now,
                    dt: 0.02,
                    ..Cues::default()
                },
                Blend::default(),
            )
        };
        let grip = |animation: &Animation, hand| {
            animation
                .layers
                .iter()
                .filter(|layer| layer.mask == Mask::Fingers(hand) && layer.weight > 0.5)
                .map(|layer| layer.clip)
                .next()
        };
        let loaded = Some(CrossbowCarry {
            bolt: Some(Bolt::Groove),
            ..CrossbowCarry::default()
        });
        // Carried low out of combat and on the move, aimed standing in combat; loaded, both
        // hands on the stock.
        let idle = frame(&mut Playback::default(), 0., false, 0.);
        assert_eq!(idle.upper.0, "Crossbow_Idle_Loop");
        assert_eq!(idle.carry.crossbow, loaded);
        assert_eq!(grip(&idle, Hand::Left), Some("Grip_Crossbow_L"));
        assert_eq!(grip(&idle, Hand::Right), Some("Grip_Crossbow_R"));
        assert_eq!(
            frame(&mut Playback::default(), 2., true, 0.).upper.0,
            "Crossbow_Idle_Loop"
        );
        assert_eq!(
            frame(&mut Playback::default(), 0., true, 0.).upper.0,
            "Crossbow_Aim_Loop"
        );
        // The shot looses the bolt, then the reload winds, hands off, draws and lays a bolt.
        let mut playback = Playback::default();
        frame(&mut playback, 0., true, 0.);
        playback.strike("Crossbow_Fire", 0.4, Region::Upper, 0.02);
        playback.recover("Crossbow_Reload", 1.6);
        let at = |playback: &Playback, time: f64| {
            let mut playback = playback.clone();
            let mut now = 0.02;
            let mut last = None;
            while now <= 0.02 + time + 1e-9 {
                last = Some(frame(&mut playback, 0., true, now));
                now += 0.02;
            }
            last.expect("frames")
        };
        let aimed = at(&playback, 0.04);
        assert_eq!(aimed.upper.0, "Crossbow_Fire");
        assert_eq!(aimed.carry.crossbow, loaded);
        let loosed = at(&playback, 0.2);
        assert_eq!(loosed.carry.crossbow, Some(CrossbowCarry::default()));
        let winding = at(&playback, 0.4 + 0.5);
        assert_eq!(winding.upper.0, "Crossbow_Reload");
        assert!(
            winding
                .carry
                .crossbow
                .is_some_and(|c| c.crank && !c.left_hand && c.bolt.is_none())
        );
        assert_eq!(grip(&winding, Hand::Left), Some(CRANK_GRIP));
        let drawing = at(&playback, 0.4 + 1.2);
        assert_eq!(
            drawing.carry.crossbow,
            Some(CrossbowCarry {
                left_hand: true,
                crank: false,
                bolt: Some(Bolt::Hand)
            })
        );
        assert_eq!(grip(&drawing, Hand::Right), None);
        assert_eq!(grip(&drawing, Hand::Left), Some("Grip_Crossbow_L"));
        let laid = at(&playback, 0.4 + 1.4);
        assert_eq!(
            laid.carry.crossbow,
            Some(CrossbowCarry {
                left_hand: true,
                crank: false,
                bolt: Some(Bolt::Groove)
            })
        );
        let back = at(&playback, 0.4 + 1.5);
        assert_eq!(back.carry.crossbow, loaded);
        assert_eq!(grip(&back, Hand::Right), Some("Grip_Crossbow_R"));
    }

    /// The heavy running set (#1073, asset-tooling #823): a great weapon runs in its own
    /// carrying run, a `load.*` item at run speed runs on the heavy legs with the loaded upper
    /// body over them in the stride's phase, and light weapons keep the light run.
    #[test]
    fn a_great_weapon_is_drawn_from_and_sheathed_across_the_back() {
        // Without motions the weapon swaps sockets halfway through the 0.8 s clips.
        let step = |playback: &mut Playback, drawn| {
            playback.advance(
                Kind::Ranger.clips(),
                |clip| {
                    if clip.starts_with("TwoHand_") {
                        0.8
                    } else {
                        1.
                    }
                },
                Cues {
                    dt: 0.1,
                    loadout: Some(Loadout::Maul),
                    weapon_drawn: Some(drawn),
                    ..Cues::default()
                },
                Blend::default(),
            )
        };
        let mut playback = Playback::default();
        let sheathed = step(&mut playback, false);
        assert_eq!(sheathed.carry.weapon_drawn, Some(false));
        assert_ne!(sheathed.upper.0, GREAT_WEAPON_IDLE);
        // Drawing it plays the draw, the weapon staying on the back until the swap.
        let drawing = step(&mut playback, true);
        assert_eq!(drawing.upper.0, GREAT_WEAPON_DRAW);
        assert_eq!(drawing.carry.weapon_drawn, Some(false));
        let frames: Vec<_> = (0..10).map(|_| step(&mut playback, true)).collect();
        assert_eq!(frames[3].upper.0, GREAT_WEAPON_DRAW);
        assert_eq!(frames[3].carry.weapon_drawn, Some(true));
        // Then it stands in the great weapon's stance.
        assert_eq!(frames[9].upper.0, GREAT_WEAPON_IDLE);
        // Sheathing plays the sheathe; another loadout changes modes without one.
        assert_eq!(step(&mut playback, false).upper.0, GREAT_WEAPON_SHEATHE);
        let mut sword = Playback::default();
        let cues = |drawn| Cues {
            dt: 0.1,
            loadout: Some(Loadout::Sword),
            weapon_drawn: Some(drawn),
            ..Cues::default()
        };
        let duration = |_: &str| 1.;
        sword.advance(
            Kind::Ranger.clips(),
            duration,
            cues(false),
            Blend::default(),
        );
        let drawn = sword.advance(Kind::Ranger.clips(), duration, cues(true), Blend::default());
        assert_ne!(drawn.upper.0, GREAT_WEAPON_DRAW);
        assert_eq!(drawn.carry.weapon_drawn, Some(true));
    }

    #[test]
    fn great_weapons_and_loads_run_heavy() {
        let run = f64::from(mmorpg_core::RUN_SPEED_UNITS_PER_SECOND) / 100.;
        let step = |playback: &mut Playback, cues: Cues<'_>| {
            playback.advance(
                Kind::Ranger.clips(),
                |clip| if clip == HEAVY_RUN { 0.5 } else { 1. },
                Cues { dt: 0.1, ..cues },
                Blend::default(),
            )
        };
        let running = |loadout| {
            step(
                &mut Playback::default(),
                Cues {
                    speed: run,
                    loadout: Some(loadout),
                    ..Cues::default()
                },
            )
            .lower
            .0
        };
        assert_eq!(running(Loadout::Maul), "TwoHand_Run_Loop");
        assert_eq!(running(Loadout::GreatHammer), "TwoHand_Run_Loop");
        assert_eq!(running(Loadout::GreatAxe), "TwoHand_Run_Loop");
        assert_eq!(running(Loadout::Greatsword), "TwoHand_Sword_Run_Loop");
        assert_eq!(running(Loadout::Sword), "Run_Fwd_Loop");
        assert_eq!(running(Loadout::FlangedMace), "Run_Fwd_Loop");
        for &loadout in <Loadout as strum::VariantArray>::VARIANTS {
            if let Some(great) = loadout.great_weapon() {
                assert_eq!(loadout.attachments()[0].item, great.item(), "{loadout:?}");
            }
        }

        let sack = Chore {
            item: "equipment.load.sack",
            socket: "chest.carry",
            idle: Some("Carry_Idle_Loop"),
            walk: Some("Carry_Walk_Loop"),
        };
        let loaded = |speed| Cues {
            speed,
            chore: Some(sack),
            ..Cues::default()
        };
        let mut playback = Playback::default();
        let walking = step(&mut playback, loaded(1.5));
        assert_eq!(walking.lower.0, "Carry_Walk_Loop");
        assert_eq!(walking.carry.chore, Some(sack));
        let mut playback = Playback::default();
        let first = step(&mut playback, loaded(run));
        assert_eq!((first.lower.0, first.upper.0), (HEAVY_RUN, CARRY_RUN));
        assert_eq!(first.carry.chore, Some(sack));
        // The 1 s upper body keeps the 0.5 s stride's phase.
        for _ in 0..3 {
            let frame = step(&mut playback, loaded(run));
            assert_eq!((frame.lower.0, frame.upper.0), (HEAVY_RUN, CARRY_RUN));
            assert!(
                (frame.upper.1 - 2. * frame.lower.1).abs() < 1e-4,
                "{:?} against {:?}",
                frame.upper,
                frame.lower
            );
        }
        // A hand-carried item keeps its walk whatever the speed.
        let lantern = Chore {
            item: "equipment.tool.lantern",
            socket: "hand_r.carry",
            ..sack
        };
        let hand = step(
            &mut Playback::default(),
            Cues {
                speed: run,
                chore: Some(lantern),
                ..Cues::default()
            },
        );
        assert_eq!(hand.lower.0, "Carry_Walk_Loop");
    }

    /// Both fetched Rangers carry the whole heavy running set the clip choice names (#1073).
    #[test]
    fn the_fetched_rangers_carry_the_heavy_running_set() {
        for sex in ["male", "female"] {
            let Some(json) = crate::fetched_bundles::read(&format!(
                "characters/humanoid.ranger.{sex}/humanoid.ranger.{sex}.default.humanoid-clips.json"
            )) else {
                continue;
            };
            let motions = Motions::parse(&json).expect("valid descriptor");
            let descriptor: serde_json::Value = serde_json::from_str(&json).expect("JSON");
            let names: Vec<_> = descriptor["clips"]
                .as_array()
                .expect("clips")
                .iter()
                .filter_map(|clip| clip["name"].as_str())
                .collect();
            let great = <GreatWeapon as strum::VariantArray>::VARIANTS;
            let clips = great
                .iter()
                .flat_map(|great| [great.run(), great.run_strike()])
                .chain([HEAVY_RUN, CARRY_RUN, GREAT_WEAPON_GUARD, GREAT_WEAPON_IDLE]);
            for clip in clips {
                assert!(names.contains(&clip), "the {sex} Ranger lacks {clip}");
            }
            // The two-handed family's authored strikes contact within 50 ms of the zone's
            // contact tick (#1073), so the fit barely stretches them; a jumping strike lands on
            // touchdown instead.
            let row = mmorpg_core::FamilyMelee::of(WeaponFamily::TwoHanded);
            for variant in mmorpg_core::StrikeVariant::ALL {
                let [authored, _] = melee_clips(WeaponFamily::TwoHanded, variant);
                assert!(
                    names.contains(&authored),
                    "the {sex} Ranger lacks {authored}"
                );
                if variant == mmorpg_core::StrikeVariant::Jumping {
                    continue;
                }
                let contact = motions
                    .get(authored)
                    .and_then(|m| m.contact)
                    .expect("contact");
                let zone =
                    f64::from(row.strike(variant).contact_ticks) / f64::from(mmorpg_core::TICK_HZ);
                assert!(
                    (contact - zone).abs() < 0.05,
                    "{authored}: {contact} vs {zone}"
                );
            }
            for great in great {
                let strike = motions.get(great.run_strike()).expect("strike");
                let contact = strike
                    .contact
                    .expect("a running strike records its contact");
                assert!(
                    (0.55..0.65).contains(&contact),
                    "{} at {contact}",
                    great.run_strike()
                );
            }
        }
    }
}
