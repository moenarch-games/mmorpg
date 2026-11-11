//! One mouse owner at a time (#636, Atlas `5.2.i636-2`): which game part reads mouse motion
//! and what the left and right buttons mean right now. Both clients run this one state machine;
//! it holds no binding (`input-bindings` owns keys and held lifecycle) and decides no outcome
//! (the zone resolves every strike, guard and selection).
//!
//! | Owner | Mouse motion | Left click | Right click |
//! | --- | --- | --- | --- |
//! | Free look | Camera and heading | Select under crosshair | Cancels a cast |
//! | Focus | Cursor; camera tracks target | Select or click UI | Cancels a cast |
//! | Strike or guard hold | Direction and lane | Nothing | Cancel, no attack |
//! | Manual aim | Aim | Confirm | Cancel, no attack |
//! | Wheel | Pick a sector | Confirm | Cancel, no action |
//!
//! A wheel or aim cannot open during a hold, and a hold cannot start during an aim or wheel.
//! Sword against shield is last-pressed-wins (5.2.37). Stun, death, focus loss, a menu or a
//! panel end the current gesture without firing.
use mmorpg_core::{AuraKind, StrikeAim, StrikeHeight, StrikeSide, ZoneSnapshot};

/// Pointer travel (pixels) from the press before a gesture leaves the centre or middle lane.
pub const GESTURE_THRESHOLD_PX: f64 = 24.0;

/// A held directional input with its own key (G strike, Z guard).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hold {
    Strike,
    Guard,
}

/// A temporary owner that takes the mouse from free look or focus.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Gesture {
    Hold(Hold),
    /// Manual aim of a free-targeted attack (#649); confirmed by a left click.
    Aim,
    /// A radial menu (#653); a left click confirms the sector.
    Wheel,
}

/// Who reads mouse motion now.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MouseOwner {
    FreeLook,
    Focus,
    Gesture(Gesture),
}

/// Whether the camera keeps following the mouse during a strike or guard hold
/// (Atlas `5.2.i636-4`: both are selectable, to compare in play).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum HoldCamera {
    /// The camera holds still; motion only aims the gesture.
    #[default]
    Locked,
    /// Motion aims the gesture and still turns the free-look camera.
    Free,
}

impl HoldCamera {
    /// The settings choice `controls.hold_camera`: `"free"`, otherwise locked.
    #[must_use]
    pub fn from_setting(value: &str) -> Self {
        if value == "free" {
            Self::Free
        } else {
            Self::Locked
        }
    }
}

/// What a left click means under the current owner.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LeftClick {
    /// Free look selects under the crosshair, focus under the cursor (or clicks the UI).
    Select,
    /// An aim or wheel confirms and ends.
    Confirm(Gesture),
    /// A strike or guard hold ignores the left button.
    Nothing,
}

/// What a right click means under the current owner.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RightClick {
    /// No gesture: right click cancels the viewer's cast (the zone ignores it otherwise).
    CancelCast,
    /// The gesture ended without firing.
    Cancel(Gesture),
}

/// Starting a hold.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Begin {
    /// An aim or wheel owns the mouse; the hold does not start.
    Refused,
    /// The hold owns the mouse; a preempted other hold ended without firing.
    Started { preempted: Option<Hold> },
}

/// A deliberate key release of the owning hold.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HoldRelease {
    /// Send one directed strike with this aim; the zone decides what it covers.
    Strike(StrikeAim),
    /// Lower the guard.
    LowerGuard,
}

/// The mouse owner state machine shared by native and browser.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct MouseOwners {
    focus: bool,
    gesture: Option<Gesture>,
    hold_camera: HoldCamera,
    /// Pointer travel since the gesture began, clamped; screen y grows downward.
    offset: [f64; 2],
}

impl MouseOwners {
    #[must_use]
    pub fn owner(&self) -> MouseOwner {
        match self.gesture {
            Some(gesture) => MouseOwner::Gesture(gesture),
            None if self.focus => MouseOwner::Focus,
            None => MouseOwner::FreeLook,
        }
    }

    #[must_use]
    pub fn gesture(&self) -> Option<Gesture> {
        self.gesture
    }

    /// Follows the combat focus (a selected opponent the camera tracks).
    pub fn set_focus(&mut self, focus: bool) {
        self.focus = focus;
    }

    #[must_use]
    pub fn hold_camera(&self) -> HoldCamera {
        self.hold_camera
    }

    pub fn set_hold_camera(&mut self, hold_camera: HoldCamera) {
        self.hold_camera = hold_camera;
    }

    fn start(&mut self, gesture: Gesture) {
        self.gesture = Some(gesture);
        self.offset = [0.0; 2];
    }

    /// The hold's key went down. The most recent of strike and guard owns the mouse (5.2.37).
    pub fn begin_hold(&mut self, hold: Hold) -> Begin {
        let preempted = match self.gesture {
            Some(Gesture::Aim | Gesture::Wheel) => return Begin::Refused,
            Some(Gesture::Hold(held)) if held != hold => Some(held),
            _ => None,
        };
        self.start(Gesture::Hold(hold));
        Begin::Started { preempted }
    }

    /// `hold`'s key went up deliberately. A release of a hold that no longer owns the mouse
    /// (preempted, cancelled or interrupted) does nothing.
    pub fn release_hold(&mut self, hold: Hold) -> Option<HoldRelease> {
        if self.gesture != Some(Gesture::Hold(hold)) {
            return None;
        }
        let aim = self.strike_aim();
        self.gesture = None;
        Some(match (hold, aim) {
            (Hold::Strike, Some(aim)) => HoldRelease::Strike(aim),
            _ => HoldRelease::LowerGuard,
        })
    }

    /// Starts a manual aim; only from free look or focus.
    pub fn begin_aim(&mut self) -> bool {
        self.begin_free(Gesture::Aim)
    }

    /// Opens a wheel; only from free look or focus, never during a hold or aim.
    pub fn begin_wheel(&mut self) -> bool {
        self.begin_free(Gesture::Wheel)
    }

    fn begin_free(&mut self, gesture: Gesture) -> bool {
        if self.gesture.is_some() {
            return false;
        }
        self.start(gesture);
        true
    }

    pub fn left_click(&mut self) -> LeftClick {
        match self.gesture {
            None => LeftClick::Select,
            Some(Gesture::Hold(_)) => LeftClick::Nothing,
            Some(gesture) => {
                self.gesture = None;
                LeftClick::Confirm(gesture)
            }
        }
    }

    pub fn right_click(&mut self) -> RightClick {
        match self.gesture.take() {
            None => RightClick::CancelCast,
            Some(gesture) => RightClick::Cancel(gesture),
        }
    }

    /// Focus loss, a menu, a panel, a lost pointer lock or a retired key: the gesture ends
    /// without firing. Returns the ended gesture.
    pub fn interrupt(&mut self) -> Option<Gesture> {
        self.gesture.take()
    }

    /// Follows a projection: death or a stun ends the gesture without firing.
    pub fn observe(&mut self, snapshot: &ZoneSnapshot) -> Option<Gesture> {
        let stunned = snapshot
            .auras
            .iter()
            .any(|aura| aura.kind == AuraKind::Stun);
        if snapshot.viewer.dead || stunned {
            self.interrupt()
        } else {
            None
        }
    }

    /// Mouse motion in pixels. A gesture accumulates it; returns whether the camera turns too.
    pub fn motion(&mut self, dx: f64, dy: f64) -> bool {
        let Some(gesture) = self.gesture else {
            return !self.focus;
        };
        let limit = 2.0 * GESTURE_THRESHOLD_PX;
        for (axis, delta) in self.offset.iter_mut().zip([dx, dy]) {
            if delta.is_finite() {
                *axis = (*axis + delta).clamp(-limit, limit);
            }
        }
        match gesture {
            Gesture::Hold(_) => self.hold_camera == HoldCamera::Free && !self.focus,
            // Aiming steers the same shoulder camera.
            Gesture::Aim => true,
            Gesture::Wheel => false,
        }
    }

    /// Pointer travel since the gesture began, clamped to twice the threshold.
    #[must_use]
    pub fn offset(&self) -> [f64; 2] {
        self.offset
    }

    /// A held strike's side and height: right of the press strikes right, up strikes high.
    #[must_use]
    pub fn strike_aim(&self) -> Option<StrikeAim> {
        (self.gesture == Some(Gesture::Hold(Hold::Strike))).then(|| {
            let [dx, dy] = self.offset;
            let side = if dx <= -GESTURE_THRESHOLD_PX {
                StrikeSide::Left
            } else if dx >= GESTURE_THRESHOLD_PX {
                StrikeSide::Right
            } else {
                StrikeSide::Centre
            };
            StrikeAim::new(side, height(dy))
        })
    }

    /// A held guard's lane: up guards high.
    #[must_use]
    pub fn guard_lane(&self) -> Option<StrikeHeight> {
        (self.gesture == Some(Gesture::Hold(Hold::Guard))).then(|| height(self.offset[1]))
    }
}

fn height(dy: f64) -> StrikeHeight {
    if dy <= -GESTURE_THRESHOLD_PX {
        StrikeHeight::High
    } else if dy >= GESTURE_THRESHOLD_PX {
        StrikeHeight::Low
    } else {
        StrikeHeight::Middle
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::snapshot;
    use mmorpg_core::{AuraView, ability::ids};

    #[test]
    fn the_table_maps_clicks_and_motion_per_owner() {
        let mut owners = MouseOwners::default();
        assert_eq!(owners.owner(), MouseOwner::FreeLook);
        assert!(owners.motion(5.0, 0.0), "free look turns the camera");
        assert_eq!(owners.left_click(), LeftClick::Select);
        assert_eq!(owners.right_click(), RightClick::CancelCast);
        owners.set_focus(true);
        assert_eq!(owners.owner(), MouseOwner::Focus);
        assert!(!owners.motion(5.0, 0.0), "focus frees the cursor");
        assert_eq!(owners.left_click(), LeftClick::Select);
        assert!(owners.begin_aim());
        assert!(!owners.begin_wheel(), "no wheel during an aim");
        assert_eq!(owners.begin_hold(Hold::Strike), Begin::Refused);
        assert_eq!(owners.left_click(), LeftClick::Confirm(Gesture::Aim));
        assert_eq!(owners.owner(), MouseOwner::Focus);
        assert!(owners.begin_wheel());
        assert_eq!(owners.right_click(), RightClick::Cancel(Gesture::Wheel));
    }

    #[test]
    fn a_hold_aims_from_motion_and_fires_only_on_its_own_release() {
        let mut owners = MouseOwners::default();
        assert_eq!(
            owners.begin_hold(Hold::Strike),
            Begin::Started { preempted: None }
        );
        assert!(!owners.motion(30.0, -30.0), "the locked camera holds still");
        assert_eq!(owners.left_click(), LeftClick::Nothing);
        assert!(!owners.begin_wheel(), "no wheel during a hold");
        assert_eq!(
            owners.release_hold(Hold::Strike),
            Some(HoldRelease::Strike(StrikeAim::new(
                StrikeSide::Right,
                StrikeHeight::High
            )))
        );
        assert_eq!(owners.release_hold(Hold::Strike), None, "no second strike");
        owners.set_hold_camera(HoldCamera::Free);
        owners.begin_hold(Hold::Guard);
        assert!(owners.motion(0.0, 100.0), "the free camera keeps turning");
        assert_eq!(owners.guard_lane(), Some(StrikeHeight::Low));
        assert_eq!(
            owners.release_hold(Hold::Guard),
            Some(HoldRelease::LowerGuard)
        );
    }

    #[test]
    fn preemption_cancels_and_interruptions_never_fire() {
        let mut owners = MouseOwners::default();
        owners.begin_hold(Hold::Strike);
        assert_eq!(
            owners.begin_hold(Hold::Guard),
            Begin::Started {
                preempted: Some(Hold::Strike)
            }
        );
        assert_eq!(owners.release_hold(Hold::Strike), None, "a preempted key");
        assert_eq!(
            owners.right_click(),
            RightClick::Cancel(Gesture::Hold(Hold::Guard))
        );
        assert_eq!(owners.release_hold(Hold::Guard), None, "a cancelled key");
        owners.begin_hold(Hold::Strike);
        assert_eq!(owners.interrupt(), Some(Gesture::Hold(Hold::Strike)));
        assert_eq!(owners.release_hold(Hold::Strike), None);
        owners.begin_hold(Hold::Strike);
        let mut stunned = snapshot(1, 1);
        assert_eq!(owners.observe(&stunned), None);
        stunned.auras.push(AuraView {
            ability: ids::CONCUSSIVE_SHOT,
            kind: AuraKind::Stun,
            remaining: 10,
            amount: 0,
        });
        assert_eq!(owners.observe(&stunned), Some(Gesture::Hold(Hold::Strike)));
        owners.begin_hold(Hold::Guard);
        let mut dead = snapshot(1, 2);
        dead.viewer.dead = true;
        assert_eq!(owners.observe(&dead), Some(Gesture::Hold(Hold::Guard)));
        assert_eq!(owners.release_hold(Hold::Guard), None);
    }
}
