//! Point lights the scenery content implies (#529): every campfire's flame, every lamp post's
//! glass and the market square's lantern post. Renderer-neutral; each client maps them onto
//! its own light type. Presentation only.
use mmorpg_scenery::Scenery;

use crate::{Motion, StaticScene, geometry::point, props, settlement_props};

/// One omnidirectional light in metres.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PointLight {
    pub position: [f64; 3],
    /// sRGB colour.
    pub color: u32,
    /// Luminous power in lumens.
    pub lumens: f64,
    /// Distance in metres where the light has faded out.
    pub range: f64,
    /// Flame flicker phase; `None` for a steady light.
    pub flicker: Option<f64>,
}

impl PointLight {
    /// Luminous power at `seconds` on the presentation clock; reduced motion holds it steady.
    #[must_use]
    pub fn lumens_at(&self, seconds: f64, animate: bool) -> f64 {
        match self.flicker {
            Some(seed) if animate => {
                let wobble = 0.12 * (seconds * 9.1 + seed).sin()
                    + 0.08 * (seconds * 13.7 + seed * 2.).sin() * (seconds * 3.1).sin();
                self.lumens * (1. + wobble)
            }
            _ => self.lumens,
        }
    }
}

/// A campfire: warm, 1 800 lm, 0.45 m above its embers, out by 16 m.
pub const CAMPFIRE: PointLight = PointLight {
    position: [0.; 3],
    color: 0xff9a4a,
    lumens: 1_800.,
    range: 16.,
    flicker: Some(0.),
};

/// A lamp post's or the lantern post's lamp: 900 lm, out by 12 m.
pub const LAMP: PointLight = PointLight {
    position: [0.; 3],
    color: 0xffc27a,
    lumens: 900.,
    range: 12.,
    flicker: None,
};

/// The settlement lantern post's lamp above its base, metres.
const LANTERN_LAMP: [f64; 3] = [0., 2.75, 0.];

/// Every light of `scenery`'s campfires (from `scene`'s flames), lamp posts and lantern posts.
#[must_use]
pub fn point_lights(scene: &StaticScene, scenery: &Scenery) -> Vec<PointLight> {
    let fires = scene.animated.iter().filter_map(|part| match part.motion {
        Motion::Flame { base, seed } => Some(PointLight {
            position: [base[0], base[1] + 0.45, base[2]],
            flicker: Some(seed),
            ..CAMPFIRE
        }),
        _ => None,
    });
    let lamps = props::lamp_lights(scenery, &scene.grid)
        .into_iter()
        .map(|position| PointLight { position, ..LAMP });
    let lanterns = settlement_props::frames(scenery)
        .into_iter()
        .filter(|(entry, _)| *entry == "settlement.props.lantern-post")
        .map(|(_, frame)| PointLight {
            position: point(frame, LANTERN_LAMP),
            ..LAMP
        });
    fires.chain(lamps).chain(lanterns).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn greyhaven_campfires_lamps_and_its_lantern_post_cast_light() {
        let scenery = mmorpg_scenery::greyhaven_vale_scenery();
        let scene = StaticScene::new(&scenery);
        let lights = point_lights(&scene, &scenery);
        let fires = lights.iter().filter(|l| l.flicker.is_some()).count();
        let lamps = scenery
            .props
            .iter()
            .filter(|p| p.kind == mmorpg_scenery::PropKind::Lamp)
            .count();
        assert!(fires > 0 && lamps > 0, "{lights:?}");
        assert_eq!(lights.len(), fires + lamps + 1, "one lantern post");
        let fire = lights.iter().find(|l| l.flicker.is_some()).unwrap();
        assert_ne!(fire.lumens_at(1.3, true), fire.lumens);
        assert_eq!(fire.lumens_at(1.3, false), fire.lumens);
    }
}
