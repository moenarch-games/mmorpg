//! Flat rectangles for the native combat HUD: the cast bar, the resource bar
//! and the action bar. What they show is `mmorpg_client_model::hud`, shared
//! with the browser (#543); only the drawing lives here.
use mmorpg_client_model::hud::{CombatHud, CooldownKind};
use mmorpg_core::ResourceKind;

/// Screen rectangles for the renderer, back to front: the cast bar, the
/// resource bar and the action bar, centred above the bottom edge.
#[must_use]
pub fn rects(hud: &CombatHud) -> Vec<HudRect> {
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
    if let Some(cast) = hud.cast {
        let color = if cast.channel { CHANNEL } else { CAST };
        bar(&mut rects, CAST_TOP, BAR_HEIGHT, cast.fill, color);
    }
    if let (Some(fill), Some(resource)) = (hud.resource_fill(), hud.resource) {
        bar(
            &mut rects,
            RESOURCE_TOP,
            BAR_HEIGHT,
            fill,
            resource_color(resource.kind),
        );
    }
    let count = hud.slots.len() as f32;
    let row = count * SLOT_SIZE + (count - 1.0).max(0.0) * SLOT_GAP;
    for (index, slot) in hud.slots.iter().enumerate() {
        let left = 0.5 - row / 2.0 + index as f32 * (SLOT_SIZE + SLOT_GAP);
        let (min, max) = ([left, SLOT_TOP], [left + SLOT_SIZE, SLOT_TOP + SLOT_SIZE]);
        let frame = if slot.casting { CASTING_FRAME } else { FRAME };
        rects.push(HudRect::new(min, max, frame));
        let inner = (
            [min[0] + BORDER, min[1] + BORDER],
            [max[0] - BORDER, max[1] - BORDER],
        );
        let face = if hud.dead { DEAD_SLOT } else { READY_SLOT };
        rects.push(HudRect::new(inner.0, inner.1, face));
        if let Some(cooldown) = slot.cooldown {
            // The covered share of the slot drains from the top as the wait runs out.
            let color = match cooldown.kind {
                CooldownKind::Ability => COOLDOWN,
                CooldownKind::Global => GLOBAL,
            };
            let bottom = inner.1[1];
            let top = bottom - (bottom - inner.0[1]) * cooldown.share();
            rects.push(HudRect::new([inner.0[0], top], inner.1, color));
        }
    }
    rects
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
        ResourceKind::Faith => [0.95, 0.85, 0.55],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mmorpg_client_model::hud::{CastBar, SlotCooldown, SlotState};
    use mmorpg_core::ResourceView;

    #[test]
    fn a_full_hud_fits_the_rect_budget_and_the_window() {
        let slot = SlotState {
            ability: 9,
            learned: true,
            unlock_level: 1,
            resource_short: false,
            out_of_range: false,
            casting: true,
            cooldown: Some(SlotCooldown {
                kind: CooldownKind::Ability,
                remaining: 10,
                total: 20,
            }),
        };
        let hud = CombatHud {
            resource: Some(ResourceView {
                kind: ResourceKind::Mana,
                value: 50,
                max: 100,
            }),
            cast: Some(CastBar {
                ability: 9,
                fill: 1.0 / 6.0,
                remaining: 50,
                channel: false,
            }),
            slots: vec![slot; 4],
            dead: false,
        };
        let rects = rects(&hud);
        assert_eq!(rects.len(), MAX_HUD_RECTS);
        for rect in rects {
            for axis in 0..2 {
                assert!(0.0 <= rect.min[axis] && rect.min[axis] <= rect.max[axis]);
                assert!(rect.max[axis] <= 1.0);
            }
        }
    }
}
