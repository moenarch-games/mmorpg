//! Cosmetic secondary motion of loose gear (Atlas 6.6.39, #498): hanging items swing and settle
//! a little behind the body when it starts, stops, turns or is knocked about.
//!
//! One damped pendulum per unit, driven by its presented root's horizontal acceleration and yaw
//! acceleration, both read in the unit's own frame, with a small jolt when the unit starts a
//! strike or takes a hit. The swing reaches the items on [`hangs`] sockets, and through three
//! cascaded springs the accessory-bone chains the bundles declare for hair, beards, skirts,
//! aprons and capes ([`chains_of`], asset-tooling #391): a chain's joint `d` turns by
//! `chain[d]`, in the unit's own frame, about the joint's *sampled* rotation. So the spring's
//! rest target is whatever both clients' clip mixers put on the joint, which is the pose a clip
//! keys on it (`secondaryPose`: kneel, sit, sleep, ride, mantle, climb and swim keep long skirts
//! out of the thighs) weighted by the clip's blend weight, and the bind pose when no clip keys
//! it; the spring's own swing stays bounded around that target by the chain's declared limit.
//! Distant actors fade to the authored rest pose and skip the work (Atlas 6.6.26); a teleport,
//! a reappearance after an interest gap and an outfit change start from rest. Presentation
//! only: it reads the sampled pose and never feeds back.

use serde_json::Value;
use std::collections::BTreeMap;

/// Natural angular frequency in radians per second (about 2.2 Hz): a strapped quiver, not a
/// free pendulum.
const STIFFNESS: f32 = 14.;
/// Damping ratio: a delayed sway that settles after one or two visible swings.
const DAMPING: f32 = 0.45;
/// Tilt per unit acceleration as a fraction of a free pendulum's (`a / g`): the strap holds most.
const RESPONSE: f32 = 0.45;
/// Roll per radian per second squared of yaw acceleration (a turn on the spot).
const TURN_RESPONSE: f32 = 0.006;
const GRAVITY: f32 = 9.81;
/// The largest swing either way in radians (about 10 degrees), so gear never leaves the back.
pub const MAX_ANGLE: f32 = 0.18;
/// Acceleration beyond this (metres per second squared) is clamped: snapshot corrections and
/// knockbacks nudge, they do not whip.
const MAX_ACCELERATION: f32 = 25.;
/// How far behind the presented velocity the spring's reference velocity lags, in seconds.
const VELOCITY_LAG_SECONDS: f64 = 0.15;
/// A presented root moving farther than this in one frame (metres) teleported or was corrected:
/// the gear starts again from rest.
pub const TELEPORT_METRES: f64 = 2.5;
/// Full detail up to this distance from the viewer (metres).
pub const FULL_METRES: f64 = 25.;
/// The swing fades out between [`FULL_METRES`] and this distance; beyond it the gear rests as
/// authored and the spring does no work.
pub const FADE_METRES: f64 = 35.;
/// The longest step integrated at once, in seconds; a longer frame is split.
const STEP_SECONDS: f32 = 1. / 60.;
/// A frame longer than this (seconds) is a hitch: integrated as this long.
const MAX_FRAME_SECONDS: f32 = 0.1;

/// The joints along a chain that sway on their own; anything longer follows its last one.
pub const CHAIN_DEPTH: usize = 3;
/// Natural frequency of each joint's spring along a chain, in radians per second (about 2 Hz at
/// the root, 1.3 Hz at the tip): each joint trails the one above it.
const CHAIN_STIFFNESS: [f32; CHAIN_DEPTH] = [12., 10., 8.];
/// Damping ratio along a chain: cloth and locks settle after one or two visible swings.
const CHAIN_DAMPING: [f32; CHAIN_DEPTH] = [0.42, 0.38, 0.34];
/// How much of a [`hangs`] item's swing a chain's root takes as its share of the chain's bound.
const CHAIN_FOLLOW: f32 = 0.7;
/// The steady trailing of a chain per metre per second of the unit's speed, as a share of its
/// bound: a walk carries a skirt or cape a little back, a run farther.
const CHAIN_DRAG: f32 = 0.12;
/// The rate (share of the bound per second) a strike or hit reaction adds at a chain's root.
const CHAIN_JOLT: f32 = 2.2;
/// The rate (radians per second) a strike or hit reaction adds to hanging gear's swing.
const GEAR_JOLT: f32 = 0.5;
/// A chain joint within this share of its bound, and nearly still, has settled.
const SETTLED: f32 = 5e-4;
/// A chain the bundles declare no limit for sways this far either way, in total (degrees): the
/// skirts' declared limit.
pub const DEFAULT_CHAIN_LIMIT_DEGREES: f32 = 20.;

/// Whether an item on the `humanoid-sockets` socket `id` hangs loose and sways: the stowed
/// quiver, bow, shield and hip weapon. Held items follow the hands and worn plates the body.
#[must_use]
pub fn hangs(socket: &str) -> bool {
    matches!(
        socket,
        "back.quiver" | "back.bow.stowed" | "back.stowed" | "hip_l.stowed"
    )
}

/// The swing to apply this frame, in radians in the model's own frame (+Z forward, +X to the
/// side, +Y up): hanging items turn by `pitch` about +X, then `roll` about +Z, around their grip.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Swing {
    pub pitch: f32,
    pub roll: f32,
    /// Each chain joint's `[pitch, roll]` by its depth from the root, as a share of the chain's
    /// bound: see [`chain_turn`].
    pub chain: [[f32; 2]; CHAIN_DEPTH],
}

impl Swing {
    /// The swing as a unit quaternion `[x, y, z, w]` in the model frame: roll × pitch.
    #[must_use]
    pub fn quaternion(self) -> [f32; 4] {
        let (sp, cp) = (self.pitch * 0.5).sin_cos();
        let (sr, cr) = (self.roll * 0.5).sin_cos();
        // q_roll(z) × q_pitch(x)
        [cr * sp, sr * sp, sr * cp, cr * cp]
    }

    #[must_use]
    pub fn is_rest(self) -> bool {
        self.pitch == 0. && self.roll == 0. && self.chain == [[0.; 2]; CHAIN_DEPTH]
    }

    /// The chain drive as `[pitch, roll]` per depth flattened, for the browser; `None` when
    /// every chain hangs as authored.
    #[must_use]
    pub fn chain_drive(self) -> Option<[f32; CHAIN_DEPTH * 2]> {
        (self.chain != [[0.; 2]; CHAIN_DEPTH]).then(|| {
            let mut drive = [0.; CHAIN_DEPTH * 2];
            for (depth, [pitch, roll]) in self.chain.iter().enumerate() {
                drive[depth * 2] = *pitch;
                drive[depth * 2 + 1] = *roll;
            }
            drive
        })
    }
}

/// One accessory-bone chain a bundle declares (`secondary`, asset-tooling #391): its joints
/// from the root and the farthest its tip swings either way, in radians.
#[derive(Clone, Debug, PartialEq)]
pub struct Chain {
    pub joints: Vec<String>,
    pub limit: f32,
}

/// The rotation of a chain's joint at `depth` this frame for `drive`, `Swing::chain[depth]`, as
/// a quaternion `[x, y, z, w]` in the model's frame: the swing shared along the `joints` the
/// chain has, so a whole chain turned together reaches at most its `limit`. Multiply it onto
/// the joint's sampled rotation, about the joint.
#[must_use]
pub fn chain_turn(drive: [f32; 2], limit: f32, joints: usize) -> [f32; 4] {
    #[allow(clippy::cast_precision_loss)]
    let share = limit / joints.max(1) as f32;
    Swing {
        pitch: drive[0] * share,
        roll: drive[1] * share,
        ..Swing::default()
    }
    .quaternion()
}

/// The chains the `appearances` (`humanoid-appearance`, one per figure) declare on their hair,
/// beards and outfits, deduplicated by root joint with the longest declaration kept: a chain
/// longer than a model's skeleton draws as the prefix of joints that model has. A missing limit
/// is the skirts' 20 degrees.
#[must_use]
pub fn chains_of(appearances: &[&Value]) -> Vec<Chain> {
    let list = |value: &'_ Value| value.as_array().cloned().unwrap_or_default();
    let declaring: Vec<Value> = appearances
        .iter()
        .flat_map(|appearance| {
            let mut entries = list(&appearance["hair"]);
            entries.extend(list(&appearance["outfits"]));
            entries.extend(list(&appearance["facialHair"]["options"]));
            entries
        })
        .collect();
    let mut by_root: BTreeMap<String, Chain> = BTreeMap::new();
    for chain in declaring
        .iter()
        .filter_map(|entry| entry["secondary"].as_array())
        .flatten()
    {
        let joints: Vec<String> = chain["joints"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .map(str::to_owned)
            .collect();
        let Some(root) = joints.first().cloned() else {
            continue;
        };
        #[allow(clippy::cast_possible_truncation)]
        let degrees = chain["swingLimitDegrees"]
            .as_f64()
            .map_or(DEFAULT_CHAIN_LIMIT_DEGREES, |d| d as f32);
        let declared = Chain {
            joints,
            limit: degrees.to_radians(),
        };
        by_root
            .entry(root)
            .and_modify(|kept| {
                if declared.joints.len() > kept.joints.len() {
                    *kept = declared.clone();
                }
            })
            .or_insert(declared);
    }
    by_root.into_values().collect()
}

/// Whether `dress`, which a unit wears now, is the one `worn` remembers; a change starts the
/// `spring` again from rest (Atlas 6.6.39).
pub fn wear<D: Copy + PartialEq>(worn: &mut D, spring: &mut Sway, dress: D) {
    if *worn != dress {
        *worn = dress;
        spring.reset();
    }
}

/// What the spring reads of one unit this frame.
#[derive(Clone, Copy, Debug)]
pub struct Motion {
    /// Presented feet in metres.
    pub feet: [f64; 3],
    /// Presented yaw in radians, 0 facing +Z, turning toward +X.
    pub yaw: f64,
    /// Distance from the viewer in metres.
    pub distance: f64,
    pub dt: f64,
    pub reduced: bool,
    /// How many strikes and reactions the unit has begun ([`crate::units::imported::Playback`]):
    /// a new one jolts the gear and chains.
    pub actions: u32,
}

/// One unit's disposable spring history. A new one starts at rest.
#[derive(Clone, Debug, Default)]
pub struct Sway {
    angle: [f32; 2],
    rate: [f32; 2],
    /// Each chain joint's `[pitch, roll]` swing and rate, by depth from the root, as a share of
    /// the chain's bound in `-1..=1`.
    chain_angle: [[f32; 2]; CHAIN_DEPTH],
    chain_rate: [[f32; 2]; CHAIN_DEPTH],
    /// The previous frame's feet and yaw.
    last: Option<([f64; 3], f64)>,
    /// The lagged horizontal velocity (metres per second) and yaw rate.
    velocity: Option<([f64; 2], f64)>,
    /// The action count last frame: a change is a jolt, the first frame only the baseline.
    actions: Option<u32>,
}

impl Sway {
    /// Forgets the history: the gear hangs at rest from the next frame on.
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    /// Advances the spring by one frame and returns the swing to draw.
    pub fn advance(&mut self, motion: Motion) -> Swing {
        let weight = detail(motion.distance);
        if motion.reduced || weight <= 0. {
            // Authored fallback: no work, no history to snap back from later.
            self.reset();
            return Swing::default();
        }
        #[allow(clippy::cast_possible_truncation)]
        let dt = motion.dt as f32;
        let Some((feet, yaw)) = self.last.replace((motion.feet, motion.yaw)) else {
            return self.swing(weight);
        };
        let moved = (motion.feet[0] - feet[0]).hypot(motion.feet[2] - feet[2]);
        if moved > TELEPORT_METRES || !moved.is_finite() {
            self.reset();
            self.last = Some((motion.feet, motion.yaw));
            return Swing::default();
        }
        if dt <= 0. || !dt.is_finite() {
            return self.swing(weight);
        }
        let seconds = f64::from(dt);
        let velocity = [
            (motion.feet[0] - feet[0]) / seconds,
            (motion.feet[2] - feet[2]) / seconds,
        ];
        let turn = wrap(motion.yaw - yaw) / seconds;
        let Some((smooth, smooth_turn)) = &mut self.velocity else {
            self.velocity = Some((velocity, turn));
            return self.swing(weight);
        };
        // Acceleration as the gap to a lagged velocity: it filters the frame-to-frame jitter of
        // interpolated snapshots that a plain difference would turn into trembling gear.
        let follow = 1. - (-seconds / VELOCITY_LAG_SECONDS).exp();
        for axis in 0..2 {
            smooth[axis] += (velocity[axis] - smooth[axis]) * follow;
        }
        *smooth_turn += (turn - *smooth_turn) * follow;
        let world = [
            (velocity[0] - smooth[0]) / VELOCITY_LAG_SECONDS,
            (velocity[1] - smooth[1]) / VELOCITY_LAG_SECONDS,
        ];
        // World acceleration into the unit's frame: forward along its facing, side toward +X.
        let (sin, cos) = motion.yaw.sin_cos();
        #[allow(clippy::cast_possible_truncation)]
        let forward = (world[0] * sin + world[1] * cos) as f32;
        #[allow(clippy::cast_possible_truncation)]
        let side = (world[0] * cos - world[1] * sin) as f32;
        #[allow(clippy::cast_possible_truncation)]
        let spin = ((turn - *smooth_turn) / VELOCITY_LAG_SECONDS) as f32;
        // The lagged velocity into the unit's frame: the chains' steady trailing.
        #[allow(clippy::cast_possible_truncation)]
        let speed_forward = (smooth[0] * sin + smooth[1] * cos) as f32;
        #[allow(clippy::cast_possible_truncation)]
        let speed_side = (smooth[0] * cos - smooth[1] * sin) as f32;
        let clamp = |a: f32| a.clamp(-MAX_ACCELERATION, MAX_ACCELERATION);
        // Gear lags the body. A positive pitch about +X swings a hanging item's bottom back
        // (-Z) and a positive roll about +Z swings it toward +X: speeding up forward swings it
        // back, sidestepping toward +X swings it toward -X, and turning toward +X swings the
        // back's gear toward +X.
        let target = [
            (clamp(forward) / GRAVITY * RESPONSE).clamp(-MAX_ANGLE, MAX_ANGLE),
            (spin * TURN_RESPONSE - clamp(side) / GRAVITY * RESPONSE).clamp(-MAX_ANGLE, MAX_ANGLE),
        ];
        let chain_target = [
            ((target[0] / MAX_ANGLE) * CHAIN_FOLLOW + speed_forward * CHAIN_DRAG).clamp(-1., 1.),
            ((target[1] / MAX_ANGLE) * CHAIN_FOLLOW - speed_side * CHAIN_DRAG).clamp(-1., 1.),
        ];
        let jolted = self
            .actions
            .replace(motion.actions)
            .is_some_and(|last| last != motion.actions);
        if jolted {
            self.rate[0] += GEAR_JOLT;
            self.chain_rate[0][0] += CHAIN_JOLT;
        }
        let mut left = dt.min(MAX_FRAME_SECONDS);
        while left > 0. {
            let step = left.min(STEP_SECONDS);
            left -= step;
            for ((angle, rate), target) in self.angle.iter_mut().zip(&mut self.rate).zip(target) {
                let accel =
                    STIFFNESS * STIFFNESS * (target - *angle) - 2. * DAMPING * STIFFNESS * *rate;
                *rate += accel * step;
                *angle = (*angle + *rate * step).clamp(-MAX_ANGLE, MAX_ANGLE);
            }
            // Each joint springs toward the one above it, the root toward the body's drive.
            for depth in 0..CHAIN_DEPTH {
                let above = if depth == 0 {
                    chain_target
                } else {
                    self.chain_angle[depth - 1]
                };
                let stiffness = CHAIN_STIFFNESS[depth];
                let damping = CHAIN_DAMPING[depth];
                for axis in 0..2 {
                    let angle = &mut self.chain_angle[depth][axis];
                    let rate = &mut self.chain_rate[depth][axis];
                    let accel = stiffness * stiffness * (above[axis] - *angle)
                        - 2. * damping * stiffness * *rate;
                    *rate += accel * step;
                    *angle = (*angle + *rate * step).clamp(-1., 1.);
                    // Settled: rest exactly, so a still chain costs the clients nothing.
                    if angle.abs() < SETTLED
                        && rate.abs() < SETTLED * 10.
                        && above[axis].abs() < SETTLED
                    {
                        *angle = 0.;
                        *rate = 0.;
                    }
                }
            }
        }
        self.swing(weight)
    }

    fn swing(&self, weight: f32) -> Swing {
        Swing {
            pitch: self.angle[0] * weight,
            roll: self.angle[1] * weight,
            chain: self
                .chain_angle
                .map(|[pitch, roll]| [pitch * weight, roll * weight]),
        }
    }
}

/// The share of the swing drawn at `distance` metres from the viewer: 1 near, fading to 0.
#[must_use]
pub fn detail(distance: f64) -> f32 {
    #[allow(clippy::cast_possible_truncation)]
    let share = ((FADE_METRES - distance) / (FADE_METRES - FULL_METRES)).clamp(0., 1.) as f32;
    if distance.is_finite() { share } else { 1. }
}

fn wrap(angle: f64) -> f64 {
    let tau = std::f64::consts::TAU;
    (angle + std::f64::consts::PI).rem_euclid(tau) - std::f64::consts::PI
}

#[cfg(test)]
mod tests {
    use super::*;

    fn motion(x: f64, yaw: f64) -> Motion {
        Motion {
            feet: [x, 0., 0.],
            yaw,
            distance: 5.,
            dt: 1. / 60.,
            reduced: false,
            actions: 0,
        }
    }

    #[test]
    fn a_stop_swings_the_gear_then_it_settles_within_bounds() {
        let mut sway = Sway::default();
        // Walking toward +X while facing +X (yaw π/2), then stopping dead.
        let yaw = std::f64::consts::FRAC_PI_2;
        let mut x = 0.;
        for _ in 0..60 {
            x += 2. / 60.;
            sway.advance(motion(x, yaw));
        }
        let mut peak = 0f32;
        for _ in 0..12 {
            let swing = sway.advance(motion(x, yaw));
            peak = peak.max(swing.pitch.abs());
            assert!(swing.pitch.abs() <= MAX_ANGLE && swing.roll.abs() < 1e-3);
        }
        assert!(peak > 0.02, "the stop swings the gear ({peak})");
        for _ in 0..120 {
            sway.advance(motion(x, yaw));
        }
        let rest = sway.advance(motion(x, yaw));
        assert!(rest.pitch.abs() < 1e-3, "settled ({rest:?})");
    }

    #[test]
    fn a_teleport_distance_and_reduced_motion_rest_as_authored() {
        let mut sway = Sway::default();
        let mut x = 0.;
        for _ in 0..30 {
            x += 3. / 60.;
            sway.advance(motion(x, 0.));
        }
        sway.advance(motion(x, 0.));
        assert!(!sway.advance(motion(x, 0.)).is_rest());
        assert!(sway.advance(motion(x + 40., 0.)).is_rest(), "teleport");
        assert!(
            sway.advance(motion(x + 40., 0.)).is_rest(),
            "no kick after it"
        );
        let far = Motion {
            distance: FADE_METRES + 1.,
            ..motion(x, 0.)
        };
        assert!(sway.advance(far).is_rest());
        let reduced = Motion {
            reduced: true,
            ..motion(x, 0.)
        };
        assert!(sway.advance(reduced).is_rest());
        assert!((detail(30.) - 0.5).abs() < 1e-6 && detail(FULL_METRES) == 1.);
    }

    /// Walks at `speed` m/s for one second toward +X, facing +X, then stops dead and settles for
    /// six: the largest chain drive it ever produced, and the swing at the end.
    fn walk_then_stop(speed: f64) -> (f32, Swing) {
        let mut sway = Sway::default();
        let yaw = std::f64::consts::FRAC_PI_2;
        let mut x = 0.;
        let mut peak = 0f32;
        let mut track = |swing: Swing| {
            for [pitch, roll] in swing.chain {
                peak = peak.max(pitch.abs()).max(roll.abs());
            }
        };
        for _ in 0..60 {
            x += speed / 60.;
            track(sway.advance(motion(x, yaw)));
        }
        for _ in 0..360 {
            track(sway.advance(motion(x, yaw)));
        }
        let last = sway.advance(motion(x, yaw));
        drop(track);
        (peak, last)
    }

    #[test]
    fn chains_swing_within_the_bound_and_settle_to_rest() {
        let (peak, last) = walk_then_stop(4.);
        assert!(peak > 0.05, "the walk moves the chains ({peak})");
        assert!(peak <= 1., "the drive stays within the bound ({peak})");
        for [pitch, roll] in last.chain {
            assert!(pitch.abs() < 1e-3 && roll.abs() < 1e-3, "{last:?}");
        }
        // Each joint's share keeps the whole chain within its limit even fully driven.
        let limit = 20f32.to_radians();
        let [x, _, _, w] = chain_turn([1., 0.], limit, 3);
        let angle = 2. * x.atan2(w);
        assert!((angle - limit / 3.).abs() < 1e-5, "{angle}");
    }

    #[test]
    fn a_fixed_timestep_replays_the_same_swing() {
        assert_eq!(walk_then_stop(3.).0, walk_then_stop(3.).0);
    }

    #[test]
    fn a_jolt_kicks_a_standing_chain_and_it_settles() {
        let mut sway = Sway::default();
        for _ in 0..30 {
            sway.advance(motion(0., 0.));
        }
        let kicked = sway.advance(Motion {
            actions: 1,
            ..motion(0., 0.)
        });
        assert!(
            kicked.chain[0][0] > 0.01,
            "the strike jolts the chain ({kicked:?})"
        );
        for _ in 0..180 {
            sway.advance(Motion {
                actions: 1,
                ..motion(0., 0.)
            });
        }
        let rest = sway.advance(Motion {
            actions: 1,
            ..motion(0., 0.)
        });
        assert!(rest.chain[0][0].abs() < 1e-3, "{rest:?}");
    }

    #[test]
    fn a_teleport_and_an_outfit_change_clear_the_chain_history() {
        let mut sway = Sway::default();
        let mut x = 0.;
        for _ in 0..30 {
            x += 4. / 60.;
            sway.advance(motion(x, 0.));
        }
        assert!(sway.chain_angle != [[0.; 2]; CHAIN_DEPTH], "swinging");
        // A teleport forgets the swing.
        assert!(sway.advance(motion(x + 40., 0.)).is_rest());
        assert_eq!(sway.chain_angle, [[0.; 2]; CHAIN_DEPTH]);
        assert_eq!(sway.chain_rate, [[0.; 2]; CHAIN_DEPTH]);
        // Another outfit starts from rest; the same one keeps swinging.
        for _ in 0..30 {
            x += 4. / 60.;
            sway.advance(motion(x, 0.));
        }
        let mut worn = 1;
        wear(&mut worn, &mut sway, 1);
        assert!(sway.chain_angle != [[0.; 2]; CHAIN_DEPTH], "same outfit");
        wear(&mut worn, &mut sway, 2);
        assert_eq!(worn, 2);
        assert_eq!(sway.chain_angle, [[0.; 2]; CHAIN_DEPTH]);
        assert_eq!(sway.chain_rate, [[0.; 2]; CHAIN_DEPTH]);
    }

    #[test]
    fn a_distant_actor_does_no_spring_work() {
        let mut sway = Sway::default();
        let far = |x: f64, actions: u32| Motion {
            distance: FADE_METRES + 1.,
            actions,
            ..motion(x, 0.)
        };
        for frame in 0..200 {
            assert!(sway.advance(far(f64::from(frame) * 0.5, frame)).is_rest());
        }
        assert_eq!(sway.chain_angle, [[0.; 2]; CHAIN_DEPTH]);
        assert_eq!(sway.chain_rate, [[0.; 2]; CHAIN_DEPTH]);
        assert_eq!(sway.rate, [0.; 2]);
        assert!(sway.last.is_none(), "no history is even started");
    }

    #[test]
    fn chains_come_from_the_declarations_and_keep_their_longest_form() {
        let appearance: Value = serde_json::json!({
            "hair": [{ "secondary": [{
                "joints": ["hair_back_01", "hair_back_02"], "swingLimitDegrees": 15
            }] }],
            "outfits": [
                { "secondary": [{ "joints": ["skirt_f_01"], "swingLimitDegrees": 20 }] },
                { "secondary": [{ "joints": ["skirt_f_01", "skirt_f_02", "skirt_f_03"] }] }
            ],
            "facialHair": { "options": [{ "secondary": [{
                "joints": ["beard_01"], "swingLimitDegrees": 12
            }] }] },
        });
        let chains = chains_of(&[&appearance]);
        let skirt = chains
            .iter()
            .find(|chain| chain.joints[0] == "skirt_f_01")
            .expect("declared");
        assert_eq!(skirt.joints.len(), 3, "the longest declaration wins");
        assert!((skirt.limit - DEFAULT_CHAIN_LIMIT_DEGREES.to_radians()).abs() < 1e-6);
        let beard = chains.iter().find(|c| c.joints[0] == "beard_01").unwrap();
        assert!((beard.limit - 12f32.to_radians()).abs() < 1e-6);
        assert_eq!(chains.len(), 3);
    }

    #[test]
    fn only_loose_sockets_hang() {
        assert!(hangs("back.quiver") && hangs("hip_l.stowed"));
        assert!(!hangs("hand_r.weapon") && !hangs("head.hair") && !hangs("spine_03.armor"));
    }
}
