//! How a melee blow feels on contact (#900): a short hitstop on both units, a camera kick for
//! the viewer and a recoil of the struck body, plus the planted feet a strike clip marks.
//!
//! All of it is presentation only. The zone's contact tick decides the hit and its timing stays
//! unchanged: the hitstop only holds the strike and reaction clips' clocks for a few frames
//! ([`super::imported::Playback::hitstop`]), the kick and the recoil only offset the drawn camera
//! and root and settle back, and the foot lock only lets the drawn root trail the projected body
//! within a bound. Reduced motion shows none of them.

use super::traversal::Motions;

/// The hitstop of the lightest and the heaviest weapons, in seconds.
pub const HITSTOP_SECONDS: [f32; 2] = [0.05, 0.09];
/// The share of their pace the struck and striking clips keep through a hitstop.
pub const HITSTOP_RATE: f32 = 0.1;
/// How long the camera kick and the recoil take to settle after the hitstop, in seconds.
pub const SETTLE_SECONDS: f64 = 0.16;
/// The camera kick of the heaviest blow, in metres.
pub const KICK_METERS: f64 = 0.05;
/// The recoil of the heaviest blow, in metres away from the attacker.
pub const RECOIL_METERS: f64 = 0.12;
/// The farthest planted feet hold the drawn root behind the projected body, in metres.
pub const FOOT_LOCK_METERS: f64 = 0.3;
/// Seconds over which the drawn root catches up once the feet lift.
pub const FOOT_SETTLE_SECONDS: f64 = 0.15;

/// How heavy the weapon swinging `clip` is, 0 (a dagger or a fist) to 1 (a two-handed
/// weapon): it scales the hitstop, the kick and the recoil.
#[must_use]
pub fn weight(clip: &str) -> f32 {
    let family = clip.split('_').next().unwrap_or_default();
    match family {
        "TwoHand" => 1.,
        "Mace" => 0.75,
        "Sword" if clip.contains("Smash") || clip.contains("Cleave") => 1.,
        "Sword" | "Spear" | "Staff" | "Shield" | "Charge" => 0.5,
        "Kick" | "Bow" => 0.25,
        _ => 0.,
    }
}

/// The hitstop of a blow by a weapon of `weight` ([`weight`]), in seconds.
#[must_use]
pub fn hitstop_seconds(weight: f32) -> f32 {
    let [light, heavy] = HITSTOP_SECONDS;
    light + (heavy - light) * weight.clamp(0., 1.)
}

/// A confirmed blow being shown: when it landed, how heavy it was and which way it pushed
/// (unit horizontal `[x, z]`, from the attacker toward the victim).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Impact {
    pub started: f64,
    pub weight: f32,
    pub away: [f64; 2],
}

impl Impact {
    /// The impact's hitstop in seconds.
    #[must_use]
    pub fn hitstop(self) -> f32 {
        hitstop_seconds(self.weight)
    }

    /// Whether the impact still moves anything at `now`.
    #[must_use]
    pub fn live(self, now: f64) -> bool {
        now < self.started + f64::from(self.hitstop()) + SETTLE_SECONDS
    }

    /// How far out the kick and the recoil are at `now`, 0 to 1: they snap out on contact,
    /// hold through the hitstop and ease back after it.
    fn envelope(self, now: f64) -> f64 {
        let after = now - self.started - f64::from(self.hitstop());
        if now < self.started || after >= SETTLE_SECONDS {
            0.
        } else if after <= 0. {
            1.
        } else {
            let left = (1. - after / SETTLE_SECONDS).max(0.);
            left * left
        }
    }

    /// The viewer's camera kick at `now`, in metres: along the blow and a little down.
    #[must_use]
    pub fn kick(self, now: f64) -> [f64; 3] {
        let size = KICK_METERS * f64::from(0.4 + 0.6 * self.weight) * self.envelope(now);
        [self.away[0] * size, -0.5 * size, self.away[1] * size]
    }

    /// The struck body's drawn recoil at `now`, in metres away from the attacker.
    #[must_use]
    pub fn recoil(self, now: f64) -> [f64; 3] {
        let size = RECOIL_METERS * f64::from(0.4 + 0.6 * self.weight) * self.envelope(now);
        [self.away[0] * size, 0., self.away[1] * size]
    }
}

/// The unit horizontal direction from `from` toward `to` (`[x, z]`), or none when they stand
/// together.
#[must_use]
pub fn away(from: [f64; 3], to: [f64; 3]) -> Option<[f64; 2]> {
    let [x, z] = [to[0] - from[0], to[2] - from[2]];
    let length = x.hypot(z);
    (length > 1e-3).then(|| [x / length, z / length])
}

/// Holds the drawn root still while the legs' strike clip marks both feet planted
/// (`strike.footPlants`, asset-tooling #741), so the feet do not slide while the projected body
/// still carries on, after a run or a jump's landing; once the clip lifts a foot the drawn root
/// catches up. It trails the projected body by at most [`FOOT_LOCK_METERS`].
#[derive(Clone, Copy, Debug, Default)]
pub struct FootLock {
    /// The drawn horizontal position the planted feet hold, `[x, z]`.
    anchor: Option<[f64; 2]>,
    offset: [f64; 2],
}

impl FootLock {
    /// The offset to draw the root by this frame, for a unit whose legs show `lower` (clip and
    /// time) with its projected feet at `feet`.
    pub fn advance(
        &mut self,
        motions: Option<&Motions>,
        lower: (&'static str, f32),
        feet: [f64; 3],
        dt: f64,
        reduced: bool,
    ) -> [f64; 3] {
        let planted = !reduced
            && motions
                .and_then(|motions| motions.get(lower.0))
                .and_then(|motion| motion.strike.as_ref())
                .is_some_and(|marks| marks.planted(f64::from(lower.1)));
        if planted {
            let anchor = *self
                .anchor
                .get_or_insert([feet[0] + self.offset[0], feet[2] + self.offset[1]]);
            let offset = [anchor[0] - feet[0], anchor[1] - feet[2]];
            let length = offset[0].hypot(offset[1]);
            let scale = if length > FOOT_LOCK_METERS {
                FOOT_LOCK_METERS / length
            } else {
                1.
            };
            self.offset = offset.map(|axis| axis * scale);
        } else {
            self.anchor = None;
            let keep = if reduced {
                0.
            } else {
                (1. - dt / FOOT_SETTLE_SECONDS).max(0.)
            };
            self.offset = self.offset.map(|axis| axis * keep);
        }
        [self.offset[0], 0., self.offset[1]]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn heavier_weapons_stop_longer() {
        assert_eq!(hitstop_seconds(weight("Dagger_Stab_R")), HITSTOP_SECONDS[0]);
        assert_eq!(
            hitstop_seconds(weight("TwoHand_Swing_A")),
            HITSTOP_SECONDS[1]
        );
        let sword = hitstop_seconds(weight("Sword_Regular_A"));
        let mace = hitstop_seconds(weight("Mace_Swing_A"));
        assert!(HITSTOP_SECONDS[0] < sword && sword < mace && mace < HITSTOP_SECONDS[1]);
    }

    #[test]
    fn the_kick_and_recoil_hold_through_the_hitstop_and_settle() {
        let impact = Impact {
            started: 1.,
            weight: 1.,
            away: [1., 0.],
        };
        assert_eq!(impact.recoil(0.9), [0.; 3]);
        assert!((impact.recoil(1.05)[0] - RECOIL_METERS).abs() < 1e-9);
        assert!(impact.kick(1.05)[1] < 0.);
        let after = 1. + f64::from(impact.hitstop()) + SETTLE_SECONDS + 1e-6;
        assert_eq!(impact.recoil(after), [0.; 3]);
        assert!(!impact.live(after));
    }

    #[test]
    fn planted_feet_hold_the_drawn_root_within_a_bound() {
        let motions = Motions::parse(
            r#"{"schema": "humanoid-clips-v1", "clips": [{"name": "Sword_Jump_Strike",
                "strike": {"contactSeconds": 0.47,
                           "footPlants": {"l": [[0.43, 1.0]], "r": [[0.43, 1.0]]}}}]}"#,
        )
        .unwrap();
        let mut lock = FootLock::default();
        let clip = "Sword_Jump_Strike";
        // In the air nothing holds; on touchdown the drawn feet stay where they landed.
        assert_eq!(
            lock.advance(Some(&motions), (clip, 0.2), [0.; 3], 0.016, false),
            [0.; 3]
        );
        lock.advance(Some(&motions), (clip, 0.5), [0.; 3], 0.016, false);
        let held = lock.advance(Some(&motions), (clip, 0.6), [0.1, 0., 0.], 0.016, false);
        assert!((held[0] + 0.1).abs() < 1e-9, "{held:?}");
        let bounded = lock.advance(Some(&motions), (clip, 0.7), [2., 0., 0.], 0.016, false);
        assert!((bounded[0] + FOOT_LOCK_METERS).abs() < 1e-9, "{bounded:?}");
        // Reduced motion draws the projected body.
        assert_eq!(
            lock.advance(Some(&motions), (clip, 0.7), [2., 0., 0.], 0.016, true),
            [0.; 3]
        );
    }
}
