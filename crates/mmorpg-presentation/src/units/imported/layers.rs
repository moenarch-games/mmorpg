//! Clip layers: which joints a layer drives, and how the body regions blend.

use super::*;

/// Which joints a clip layer drives (Atlas 6.6.45).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Mask {
    /// Every joint.
    All,
    /// The [`Clips::upper_body`] subtree: spine, arms and head.
    Upper,
    /// Every other joint: root, pelvis and legs.
    Lower,
    /// One hand's finger and thumb joints: a grip pose (asset-tooling #285). While a pose has
    /// such a layer, it alone drives that hand's fingers.
    Fingers(Hand),
}

/// Where a joint sits for the masks: in the upper body or not, and on which hand's fingers.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Joint {
    pub upper: bool,
    pub fingers: Option<Hand>,
}

impl Joint {
    /// The joint named `name`, in the upper body or not.
    #[must_use]
    pub fn new(name: &str, upper: bool) -> Self {
        Self {
            upper,
            fingers: Hand::of_finger(name),
        }
    }
}

/// The hands a pose grips: each has a [`Mask::Fingers`] layer that owns its fingers.
#[must_use]
pub fn gripped(pose: &[Layer]) -> [bool; 2] {
    let mut gripped = [false; 2];
    for layer in pose {
        if let Mask::Fingers(hand) = layer.mask {
            gripped[hand.index()] = true;
        }
    }
    gripped
}

impl Mask {
    /// The name both clients and the Action Lab read.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::All => "all",
            Self::Upper => "upper",
            Self::Lower => "lower",
            Self::Fingers(Hand::Left) => "fingers_l",
            Self::Fingers(Hand::Right) => "fingers_r",
        }
    }

    /// Whether a layer with this mask drives `joint` in a pose gripping the [`gripped`] hands:
    /// a gripped hand's fingers follow only its grip.
    #[must_use]
    pub fn drives(self, joint: Joint, gripped: [bool; 2]) -> bool {
        match (self, joint.fingers) {
            (Self::Fingers(hand), fingers) => fingers == Some(hand),
            (_, Some(hand)) if gripped[hand.index()] => false,
            (Self::All, _) => true,
            (Self::Upper, _) => joint.upper,
            (Self::Lower, _) => !joint.upper,
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
