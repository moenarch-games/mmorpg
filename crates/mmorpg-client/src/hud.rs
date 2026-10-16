//! The native combat HUD: the projected class resource, the viewer's cast or
//! channel and each action-bar slot's cooldown.
//!
//! Every value is read from the received viewer projection and the immutable
//! ability catalog; the HUD never decides whether an ability can be used. Slot
//! and cast rules mirror the browser's `slotState` and `castBarView`.
use mmorpg_core::{
    ABILITY_CATALOG, AbilityId, AbilityUser, CastView, GLOBAL_COOLDOWN_TICKS, PlayerClass,
    ResourceKind, ResourceView, ZoneSnapshot, ability_by_id,
};

/// Which wait a slot shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CooldownKind {
    /// The ability's own cooldown.
    Ability,
    /// The global cooldown shared by every slot.
    Global,
}

/// A running wait on a slot, in ticks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SlotCooldown {
    pub kind: CooldownKind,
    pub remaining: u16,
    pub total: u16,
}

/// One action-bar slot.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SlotHud {
    pub ability: u8,
    /// The viewer is casting or channelling this slot's ability.
    pub casting: bool,
    pub cooldown: Option<SlotCooldown>,
}

/// What the combat HUD shows for one received projection.
#[derive(Clone, Debug, PartialEq)]
pub struct CombatHud {
    pub resource: Option<ResourceView>,
    pub cast: Option<CastView>,
    /// The projected class kit in catalog order; empty without a class.
    pub slots: Vec<SlotHud>,
    pub dead: bool,
}

/// The class abilities in catalog id order: action-bar slots 1–4.
#[must_use]
pub fn class_kit(class: PlayerClass) -> Vec<u8> {
    ABILITY_CATALOG
        .iter()
        .filter(|ability| ability.user == AbilityUser::Class(class))
        .map(|ability| ability.id.get())
        .take(4)
        .collect()
}

impl CombatHud {
    #[must_use]
    pub fn from_projection(snapshot: &ZoneSnapshot) -> Self {
        let viewer = &snapshot.viewer;
        let global = viewer.global_cooldown;
        let slots = viewer
            .class
            .map(|choice| class_kit(choice.class))
            .unwrap_or_default()
            .into_iter()
            .map(|ability| {
                let running = snapshot
                    .cooldowns
                    .iter()
                    .find(|cooldown| cooldown.ability.get() == ability)
                    .map_or(0, |cooldown| cooldown.remaining);
                // The longer wait is the one the player waits on; a tie shows
                // the ability's own cooldown.
                let cooldown = if running > 0 && running >= global {
                    Some(SlotCooldown {
                        kind: CooldownKind::Ability,
                        remaining: running,
                        total: ability_by_id(AbilityId::new(ability))
                            .map_or(running, |definition| definition.cooldown),
                    })
                } else if global > 0 {
                    Some(SlotCooldown {
                        kind: CooldownKind::Global,
                        remaining: global,
                        total: GLOBAL_COOLDOWN_TICKS,
                    })
                } else {
                    None
                };
                SlotHud {
                    ability,
                    casting: viewer
                        .cast
                        .is_some_and(|cast| cast.ability.get() == ability),
                    cooldown,
                }
            })
            .collect();
        Self {
            resource: viewer.resource,
            cast: viewer.cast,
            slots,
            dead: viewer.dead,
        }
    }

    /// The resource bar's fill, `value / max`.
    #[must_use]
    pub fn resource_fill(&self) -> Option<f32> {
        self.resource.map(|resource| {
            if resource.max == 0 {
                0.0
            } else {
                (f32::from(resource.value) / f32::from(resource.max)).clamp(0.0, 1.0)
            }
        })
    }

    /// The cast bar's fill: a cast fills, a channel drains.
    #[must_use]
    pub fn cast_fill(&self) -> Option<f32> {
        self.cast.map(|cast| {
            let progress = if cast.total == 0 {
                1.0
            } else {
                (f32::from(cast.elapsed) / f32::from(cast.total)).clamp(0.0, 1.0)
            };
            if cast.channel {
                1.0 - progress
            } else {
                progress
            }
        })
    }

    /// Flat screen rectangles for the renderer, back to front: the cast bar,
    /// the resource bar and the action bar, centred above the bottom edge.
    #[must_use]
    pub fn rects(&self) -> Vec<HudRect> {
        let mut rects = Vec::new();
        let bar = |rects: &mut Vec<HudRect>, top: f32, height: f32, fill: f32, color| {
            let (left, right) = (0.5 - BAR_HALF_WIDTH, 0.5 + BAR_HALF_WIDTH);
            rects.push(HudRect::new([left, top], [right, top + height], FRAME));
            let inset = [left + BORDER, top + BORDER];
            let width = (right - left - 2.0 * BORDER) * fill;
            rects.push(HudRect::new(
                inset,
                [inset[0] + width, top + height - BORDER],
                color,
            ));
        };
        if let Some(fill) = self.cast_fill() {
            let color = if self.cast.is_some_and(|cast| cast.channel) {
                CHANNEL
            } else {
                CAST
            };
            bar(&mut rects, CAST_TOP, BAR_HEIGHT, fill, color);
        }
        if let (Some(fill), Some(resource)) = (self.resource_fill(), self.resource) {
            bar(
                &mut rects,
                RESOURCE_TOP,
                BAR_HEIGHT,
                fill,
                resource_color(resource.kind),
            );
        }
        let count = self.slots.len() as f32;
        let row = count * SLOT_SIZE + (count - 1.0).max(0.0) * SLOT_GAP;
        for (index, slot) in self.slots.iter().enumerate() {
            let left = 0.5 - row / 2.0 + index as f32 * (SLOT_SIZE + SLOT_GAP);
            let (min, max) = ([left, SLOT_TOP], [left + SLOT_SIZE, SLOT_TOP + SLOT_SIZE]);
            let frame = if slot.casting { CASTING_FRAME } else { FRAME };
            rects.push(HudRect::new(min, max, frame));
            let inner = (
                [min[0] + BORDER, min[1] + BORDER],
                [max[0] - BORDER, max[1] - BORDER],
            );
            let face = if self.dead { DEAD_SLOT } else { READY_SLOT };
            rects.push(HudRect::new(inner.0, inner.1, face));
            if let Some(cooldown) = slot.cooldown {
                // The covered share of the slot drains from the top as the wait runs out.
                let share = if cooldown.total == 0 {
                    1.0
                } else {
                    (f32::from(cooldown.remaining) / f32::from(cooldown.total)).clamp(0.0, 1.0)
                };
                let color = match cooldown.kind {
                    CooldownKind::Ability => COOLDOWN,
                    CooldownKind::Global => GLOBAL,
                };
                let bottom = inner.1[1];
                let top = bottom - (bottom - inner.0[1]) * share;
                rects.push(HudRect::new([inner.0[0], top], inner.1, color));
            }
        }
        rects
    }
}

/// An axis-aligned screen rectangle in normalised window coordinates
/// (0,0 top left, 1,1 bottom right) with a linear RGB colour.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HudRect {
    pub min: [f32; 2],
    pub max: [f32; 2],
    pub color: [f32; 3],
}

impl HudRect {
    const fn new(min: [f32; 2], max: [f32; 2], color: [f32; 3]) -> Self {
        Self { min, max, color }
    }
}

/// At most this many rectangles: two bars of two plus four slots of three.
pub const MAX_HUD_RECTS: usize = 16;

const BAR_HALF_WIDTH: f32 = 0.12;
const BAR_HEIGHT: f32 = 0.022;
const BORDER: f32 = 0.004;
const SLOT_SIZE: f32 = 0.06;
const SLOT_GAP: f32 = 0.01;
const SLOT_TOP: f32 = 0.9;
const RESOURCE_TOP: f32 = SLOT_TOP - 0.01 - BAR_HEIGHT;
const CAST_TOP: f32 = RESOURCE_TOP - 0.05 - BAR_HEIGHT;

const FRAME: [f32; 3] = [0.02, 0.02, 0.03];
const CASTING_FRAME: [f32; 3] = [0.95, 0.8, 0.2];
const READY_SLOT: [f32; 3] = [0.35, 0.33, 0.3];
const DEAD_SLOT: [f32; 3] = [0.12, 0.12, 0.12];
const COOLDOWN: [f32; 3] = [0.08, 0.08, 0.1];
const GLOBAL: [f32; 3] = [0.18, 0.18, 0.2];
const CAST: [f32; 3] = [0.95, 0.65, 0.15];
const CHANNEL: [f32; 3] = [0.3, 0.75, 0.95];

const fn resource_color(kind: ResourceKind) -> [f32; 3] {
    match kind {
        ResourceKind::Rage => [0.8, 0.12, 0.1],
        ResourceKind::Focus => [0.9, 0.75, 0.2],
        ResourceKind::Mana => [0.15, 0.35, 0.9],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn class_kits_hold_the_catalog_order() {
        assert_eq!(class_kit(PlayerClass::Warden), [1, 2, 3, 4]);
        assert_eq!(class_kit(PlayerClass::Ranger), [5, 6, 7, 8]);
        assert_eq!(class_kit(PlayerClass::Arcanist), [9, 10, 11, 12]);
    }

    #[test]
    fn a_full_hud_fits_the_rect_budget_and_the_window() {
        let cooldown = Some(SlotCooldown {
            kind: CooldownKind::Ability,
            remaining: 10,
            total: 20,
        });
        let hud = CombatHud {
            resource: Some(ResourceView {
                kind: ResourceKind::Mana,
                value: 50,
                max: 100,
            }),
            cast: Some(CastView {
                ability: AbilityId::new(9),
                elapsed: 10,
                total: 60,
                channel: false,
            }),
            slots: vec![
                SlotHud {
                    ability: 9,
                    casting: true,
                    cooldown,
                };
                4
            ],
            dead: false,
        };
        let rects = hud.rects();
        assert_eq!(rects.len(), MAX_HUD_RECTS);
        for rect in rects {
            for axis in 0..2 {
                assert!(0.0 <= rect.min[axis] && rect.min[axis] <= rect.max[axis]);
                assert!(rect.max[axis] <= 1.0);
            }
        }
    }
}
