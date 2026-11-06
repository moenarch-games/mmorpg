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
    let lamps = props::lamp_lights(scenery, scene.terrain.grid())
        .into_iter()
        .map(|position| PointLight { position, ..LAMP });
    let lanterns = settlement_props::frames(scenery)
        .into_iter()
        .filter(|(entry, _)| *entry == "settlement.props.lantern-post")
        .map(|(_, frame)| PointLight {
            position: point(frame, LANTERN_LAMP),
            ..LAMP
        });
    let torches = CAVE_TORCHES
        .into_iter()
        .enumerate()
        .filter(|_| scenery.greyhaven())
        .map(|(i, position)| PointLight {
            position,
            color: 0xffaf68,
            lumens: 9000.,
            range: 16.,
            flicker: Some(i as f64 * 1.7),
        });
    fires.chain(lamps).chain(lanterns).chain(torches).collect()
}

/// Torch flames in Redbrand's public cave: above head height, attached to solid walls.
pub const CAVE_TORCHES: [[f64; 3]; 5] = [
    [48.12, 13.8, -134.5],
    [67.88, 13.8, -134.7],
    [79.88, 13.8, -142.],
    [61.5, 15.1, -143.88],
    [60., 14.4, -153.6],
];

/// Cosmetic eye adaptation and sky occlusion through the cave entrance. It follows the
/// viewer's projected position, never the debug camera, and cannot influence movement.
#[must_use]
pub fn cave_interior(viewer: Option<[f64; 3]>) -> f32 {
    let Some([x, y, z]) = viewer else {
        return 0.;
    };
    let cave = mmorpg_core::greyhaven_vale::cave::GREYFANG;
    if ![x, y, z].iter().all(|v| v.is_finite()) || !(10.5..15.5).contains(&y) {
        return 0.;
    }
    let inside = cave.floors.iter().any(|(min, max)| {
        x >= f64::from(min[0]) / 100.
            && x <= f64::from(max[0]) / 100.
            && z >= f64::from(min[1]) / 100.
            && z <= f64::from(max[1]) / 100.
    });
    if !inside {
        return 0.;
    }
    let depth = ((z + 156.) / 10.).clamp(0., 1.);
    (depth * depth * (3. - 2. * depth)) as f32
}

pub(crate) fn cave_torches(b: &mut crate::batching::Batcher, scenery: &Scenery) {
    use crate::geometry::{Geometry, at, box_shape, frustum};
    if !scenery.greyhaven() {
        return;
    }
    for (index, [x, y, z]) in CAVE_TORCHES.into_iter().enumerate() {
        let [nx, nz] = match index {
            0 => [1., 0.],
            1 | 2 => [-1., 0.],
            4 => [0., -1.],
            _ => [0., 1.],
        };
        b.global("cave-torch-iron", 0x343637).add(
            &box_shape(),
            at(
                x - nx * 0.3,
                y - 0.35,
                z - nz * 0.3,
                if nx == 0. { 0.025 } else { 0.3 },
                0.025,
                if nz == 0. { 0.025 } else { 0.3 },
            ),
        );
        b.global("cave-torch-wood", 0x563d2c)
            .add(&box_shape(), at(x, y - 0.3, z, 0.045, 0.3, 0.045));
        b.global("cave-torch-iron", 0x343637).add(
            &frustum(8, 1., true, true, 0.),
            at(x, y - 0.12, z, 0.12, 0.1, 0.12),
        );
        let mut flame = Geometry::default();
        flame.add(
            &frustum(7, 0., true, true, 0.),
            at(x, y - 0.05, z, 0.10, 0.32, 0.10),
        );
        b.global("cave-torch-flame", 0xff8a2a)
            .add(&flame, crate::geometry::ID);
    }
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
