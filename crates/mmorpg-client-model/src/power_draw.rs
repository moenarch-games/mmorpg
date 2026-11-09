//! Hold-to-draw for the Ranger's power shots (#607, Atlas `5.2.i607`): a key held on Aimed
//! Shot or Piercing Shot draws the bow locally, and letting it go sends the shot. A tap fires
//! from the partial draw. Input timing only: the zone still decides whether the shot happens,
//! so a release on cooldown or out of range is an ordinary refused cast.
use mmorpg_core::{AbilityId, AuraKind, ZoneSnapshot, ability::ids, ability_by_id};

use crate::hud::class_kit;

/// Aimed Shot and Piercing Shot draw while held, wherever they sit on the bar.
#[must_use]
pub fn draws_on_hold(ability: u8) -> bool {
    let ability = AbilityId::new(ability);
    ability == ids::AIMED_SHOT || ability == ids::PIERCING_SHOT
}

/// How a held draw ended on its key's release.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DrawRelease {
    /// A deliberate release: send this ability once.
    Fire(u8),
    /// A retirement (focus loss, Settings, a context change): nothing is sent.
    Drop,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct HeldDraw {
    slot: u8,
    ability: u8,
}

/// The one draw a player can hold. Newer presses supersede it and every cancel path drops it;
/// neither ever fires.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PowerDraw {
    held: Option<HeldDraw>,
}

/// A press on slot `slot` of `ability` can start a draw: the ability is learned and still on
/// that slot, and the viewer is alive, unstunned and not casting. A cooldown does not stop a
/// draw: the shot is refused when released, as any other cast.
fn can_draw(snapshot: &ZoneSnapshot, slot: u8, ability: u8) -> bool {
    let viewer = &snapshot.viewer;
    let on_slot = viewer.class.is_some_and(|choice| {
        usize::from(slot)
            .checked_sub(1)
            .and_then(|index| class_kit(choice.class).get(index).copied())
            == Some(ability)
    });
    let learned =
        ability_by_id(AbilityId::new(ability)).is_some_and(|ability| viewer.level >= ability.level);
    on_slot
        && learned
        && !viewer.dead
        && viewer.cast.is_none()
        && !snapshot
            .auras
            .iter()
            .any(|aura| aura.kind == AuraKind::Stun)
}

impl PowerDraw {
    /// A held input (a key or button with a release) pressed `slot`, holding `ability`.
    /// `true` starts a draw and sends nothing; `false` means use the slot as usual, and any
    /// earlier draw is superseded. An open interface window never starts a draw.
    pub fn press(
        &mut self,
        slot: u8,
        ability: u8,
        snapshot: Option<&ZoneSnapshot>,
        interface_open: bool,
    ) -> bool {
        let draws = draws_on_hold(ability)
            && !interface_open
            && snapshot.is_some_and(|snapshot| can_draw(snapshot, slot, ability));
        self.held = draws.then_some(HeldDraw { slot, ability });
        draws
    }

    /// `slot`'s key went up, or its hold was `retired`. A release of any other key is ignored.
    pub fn release(&mut self, slot: u8, retired: bool) -> Option<DrawRelease> {
        let held = self.held.filter(|held| held.slot == slot)?;
        self.held = None;
        Some(if retired {
            DrawRelease::Drop
        } else {
            DrawRelease::Fire(held.ability)
        })
    }

    /// Drops the draw without firing; `true` if there was one.
    pub fn cancel(&mut self) -> bool {
        self.held.take().is_some()
    }

    /// Follows the latest projection: a draw whose ability became unusable, or that an opened
    /// interface window interrupts, is dropped; `true` if it was.
    pub fn observe(&mut self, snapshot: &ZoneSnapshot, interface_open: bool) -> bool {
        match self.held {
            Some(held) if interface_open || !can_draw(snapshot, held.slot, held.ability) => {
                self.held = None;
                true
            }
            _ => false,
        }
    }

    /// The ability being drawn, if any.
    #[must_use]
    pub fn drawing(&self) -> Option<u8> {
        self.held.map(|held| held.ability)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::snapshot;
    use mmorpg_core::{AuraView, CastView, ClassChoice, Cooldown, PlayerClass, Sex};

    const AIMED_SHOT_SLOT: u8 = 1;
    const SERPENT_STING_SLOT: u8 = 2;
    const PIERCING_SHOT_SLOT: u8 = 5;
    const AIMED_SHOT: u8 = ids::AIMED_SHOT.get();
    const PIERCING_SHOT: u8 = ids::PIERCING_SHOT.get();

    fn ranger(level: u8) -> ZoneSnapshot {
        let mut snapshot = snapshot(1, 1);
        snapshot.viewer.level = level;
        snapshot.viewer.class = Some(ClassChoice {
            class: PlayerClass::Ranger,
            sex: Sex::Male,
        });
        snapshot
    }

    fn drawn(slot: u8, ability: u8) -> PowerDraw {
        let mut draw = PowerDraw::default();
        assert!(draw.press(slot, ability, Some(&ranger(60)), false));
        draw
    }

    #[test]
    fn only_the_two_power_shots_draw_on_any_slot_they_occupy() {
        assert!(draws_on_hold(AIMED_SHOT) && draws_on_hold(PIERCING_SHOT));
        assert!(!draws_on_hold(ids::RICOCHET_SHOT.get()));
        let mut draw = PowerDraw::default();
        assert!(!draw.press(
            SERPENT_STING_SLOT,
            ids::SERPENT_STING.get(),
            Some(&ranger(60)),
            false
        ));
        assert_eq!(draw.drawing(), None);
        assert_eq!(
            drawn(PIERCING_SHOT_SLOT, PIERCING_SHOT).drawing(),
            Some(PIERCING_SHOT)
        );
    }

    #[test]
    fn a_held_press_sends_nothing_until_its_release_fires_once() {
        let mut draw = drawn(AIMED_SHOT_SLOT, AIMED_SHOT);
        assert_eq!(draw.drawing(), Some(AIMED_SHOT));
        // Another slot's key going up touches nothing.
        assert_eq!(draw.release(PIERCING_SHOT_SLOT, false), None);
        assert_eq!(
            draw.release(AIMED_SHOT_SLOT, false),
            Some(DrawRelease::Fire(AIMED_SHOT))
        );
        assert_eq!(draw.release(AIMED_SHOT_SLOT, false), None);
        assert_eq!(draw.drawing(), None);
    }

    #[test]
    fn a_tap_fires_from_the_partial_draw_at_once() {
        let mut draw = PowerDraw::default();
        assert!(draw.press(AIMED_SHOT_SLOT, AIMED_SHOT, Some(&ranger(60)), false));
        assert_eq!(
            draw.release(AIMED_SHOT_SLOT, false),
            Some(DrawRelease::Fire(AIMED_SHOT))
        );
    }

    #[test]
    fn cancelling_or_retiring_never_fires() {
        let mut draw = drawn(AIMED_SHOT_SLOT, AIMED_SHOT);
        assert!(draw.cancel());
        assert!(!draw.cancel());
        assert_eq!(draw.release(AIMED_SHOT_SLOT, false), None);
        let mut draw = drawn(AIMED_SHOT_SLOT, AIMED_SHOT);
        assert_eq!(draw.release(AIMED_SHOT_SLOT, true), Some(DrawRelease::Drop));
        assert_eq!(draw.drawing(), None);
    }

    #[test]
    fn a_newer_press_supersedes_the_draw_and_the_old_key_stays_silent() {
        let mut draw = drawn(AIMED_SHOT_SLOT, AIMED_SHOT);
        let snapshot = ranger(60);
        assert!(draw.press(PIERCING_SHOT_SLOT, PIERCING_SHOT, Some(&snapshot), false));
        assert_eq!(draw.release(AIMED_SHOT_SLOT, false), None);
        assert_eq!(draw.drawing(), Some(PIERCING_SHOT));
        assert!(!draw.press(
            SERPENT_STING_SLOT,
            ids::SERPENT_STING.get(),
            Some(&snapshot),
            false
        ));
        assert_eq!(draw.drawing(), None);
    }

    #[test]
    fn dying_stun_casting_or_a_changed_bar_drop_the_draw() {
        let kept = ranger(60);
        let mut draw = drawn(AIMED_SHOT_SLOT, AIMED_SHOT);
        assert!(!draw.observe(&kept, false));
        assert_eq!(draw.drawing(), Some(AIMED_SHOT));

        let mut dead = ranger(60);
        dead.viewer.dead = true;
        let mut stunned = ranger(60);
        stunned.auras = vec![AuraView {
            ability: ids::CONCUSSIVE_SHOT,
            kind: AuraKind::Stun,
            remaining: 10,
            amount: 0,
        }];
        let mut casting = ranger(60);
        casting.viewer.cast = Some(CastView {
            ability: ids::AIMED_SHOT,
            elapsed: 0,
            total: 10,
            channel: false,
        });
        let mut rebound = ranger(60);
        rebound.viewer.class = Some(ClassChoice {
            class: PlayerClass::Warden,
            sex: Sex::Male,
        });
        for unusable in [dead, stunned, casting, rebound] {
            let mut draw = drawn(AIMED_SHOT_SLOT, AIMED_SHOT);
            assert!(draw.observe(&unusable, false));
            assert_eq!(draw.drawing(), None);
            assert_eq!(draw.release(AIMED_SHOT_SLOT, false), None);
        }

        let mut draw = drawn(AIMED_SHOT_SLOT, AIMED_SHOT);
        assert!(draw.observe(&kept, true), "an opened window drops the draw");
    }

    #[test]
    fn a_shot_that_cannot_draw_is_an_ordinary_use() {
        let mut draw = PowerDraw::default();
        let mut dead = ranger(60);
        dead.viewer.dead = true;
        assert!(!draw.press(AIMED_SHOT_SLOT, AIMED_SHOT, Some(&dead), false));
        assert!(!draw.press(AIMED_SHOT_SLOT, AIMED_SHOT, Some(&ranger(60)), true));
        assert!(!draw.press(AIMED_SHOT_SLOT, AIMED_SHOT, None, false));
        // Below its unlock level the zone refuses it, as for every ability.
        let unlock = ability_by_id(ids::PIERCING_SHOT).unwrap().level;
        assert!(!draw.press(
            PIERCING_SHOT_SLOT,
            PIERCING_SHOT,
            Some(&ranger(unlock - 1)),
            false
        ));
        // The wrong slot for the ability does not draw either.
        assert!(!draw.press(SERPENT_STING_SLOT, AIMED_SHOT, Some(&ranger(60)), false));
    }

    #[test]
    fn a_cooldown_does_not_stop_a_draw_the_zone_refuses_the_release() {
        let mut snapshot = ranger(60);
        snapshot.viewer.global_cooldown = 30;
        snapshot.cooldowns = vec![Cooldown {
            ability: ids::AIMED_SHOT,
            remaining: 90,
        }];
        let mut draw = PowerDraw::default();
        assert!(draw.press(AIMED_SHOT_SLOT, AIMED_SHOT, Some(&snapshot), false));
        assert!(!draw.observe(&snapshot, false));
        assert_eq!(
            draw.release(AIMED_SHOT_SLOT, false),
            Some(DrawRelease::Fire(AIMED_SHOT))
        );
    }
}
