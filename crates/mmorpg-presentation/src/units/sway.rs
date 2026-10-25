//! Cosmetic secondary motion of loose gear (Atlas 6.6.39, #498): hanging items swing and settle
//! a little behind the body when it starts, stops, turns or is knocked about.
//!
//! One damped pendulum per unit, driven by its presented root's horizontal acceleration and yaw
//! acceleration, both read in the unit's own frame. The swing is bounded, settles in about a
//! second and reaches only the items on [`hangs`] sockets; hair, beards and cloth stay skinned
//! as authored, because the Ranger bundle has no accessory bones to swing them by (asset-tooling
//! follow-up of #498). Distant actors fade to the authored rest pose and skip the work
//! (Atlas 6.6.26); a teleport, a reappearance after an interest gap and an outfit change start
//! from rest. Presentation only: it reads the sampled pose and never feeds back.

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
        self.pitch == 0. && self.roll == 0.
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
}

/// One unit's disposable spring history. A new one starts at rest.
#[derive(Clone, Debug, Default)]
pub struct Sway {
    angle: [f32; 2],
    rate: [f32; 2],
    /// The previous frame's feet and yaw.
    last: Option<([f64; 3], f64)>,
    /// The lagged horizontal velocity (metres per second) and yaw rate.
    velocity: Option<([f64; 2], f64)>,
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
        let clamp = |a: f32| a.clamp(-MAX_ACCELERATION, MAX_ACCELERATION);
        // Gear lags the body. A positive pitch about +X swings a hanging item's bottom back
        // (-Z) and a positive roll about +Z swings it toward +X: speeding up forward swings it
        // back, sidestepping toward +X swings it toward -X, and turning toward +X swings the
        // back's gear toward +X.
        let target = [
            (clamp(forward) / GRAVITY * RESPONSE).clamp(-MAX_ANGLE, MAX_ANGLE),
            (spin * TURN_RESPONSE - clamp(side) / GRAVITY * RESPONSE).clamp(-MAX_ANGLE, MAX_ANGLE),
        ];
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
        }
        self.swing(weight)
    }

    fn swing(&self, weight: f32) -> Swing {
        Swing {
            pitch: self.angle[0] * weight,
            roll: self.angle[1] * weight,
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

    #[test]
    fn only_loose_sockets_hang() {
        assert!(hangs("back.quiver") && hangs("hip_l.stowed"));
        assert!(!hangs("hand_r.weapon") && !hangs("head.hair") && !hangs("spine_03.armor"));
    }
}
