//! Entity identity over the exact geometry of the successfully presented native frame.
use crate::{
    ClientError,
    world::{SceneDraw, WorldFrame},
};
use mmorpg_core::{DoorId, EntityRef};
use three_d_animation::Mat4;
use three_d_projective::picking::{CullMode, PickRay};

/// What a click lands on first.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Picked {
    Unit(EntityRef),
    Door(DoorId),
}

/// How far outside a creature's, NPC's or door leaf's projected bounds a click may
/// still select it, in logical pixels: the browser's mouse margin (#369, #370).
pub const NEAR_MISS_PIXELS: f64 = 8.0;

/// The presented frame's screen, for near-miss clicks.
#[derive(Clone, Copy, Debug)]
pub struct PickScreen {
    pub view_projection: Mat4,
    pub viewport: [u32; 2],
    /// The click, in physical pixels from the top left.
    pub pixel: [f64; 2],
    /// The allowed miss in physical pixels.
    pub margin: f64,
}

#[derive(Default)]
pub struct WorldPicking {
    world: Vec<SceneDraw>,
    bodies: Vec<(EntityRef, SceneDraw)>,
}
impl WorldPicking {
    pub fn present(&mut self, frame: &WorldFrame, bodies: &[(EntityRef, SceneDraw)]) {
        // Unit bodies are handled with their identities below. Health bars, target
        // markers and spell effects never become selectable bodies or occluders.
        self.world = frame
            .draws
            .iter()
            .filter(|d| {
                !d.id.starts_with("unit-") && !d.id.starts_with("fx-") && d.material.opacity > 0.
            })
            .cloned()
            .collect();
        self.bodies = bodies
            .iter()
            .filter(|(_, d)| d.material.opacity > 0.)
            .cloned()
            .collect();
    }

    /// The exact nearest hit wins. Only when the ray hits nothing selectable may a
    /// creature, NPC or door leaf within `screen.margin` of the click be chosen, and
    /// only one in front of whatever the ray did hit, so walls and roofs still block.
    pub fn select_frame(
        &self,
        ray: PickRay,
        screen: Option<PickScreen>,
    ) -> Result<Option<Picked>, ClientError> {
        let mut nearest = f64::INFINITY;
        let mut selected = None;
        for draw in &self.world {
            if let Some(hit) = ray.intersect_mesh(&draw.mesh, draw.model, cull(draw))?
                && hit.distance < nearest
            {
                nearest = hit.distance;
                selected = door_of(draw).map(Picked::Door);
            }
        }
        for (entity, draw) in &self.bodies {
            if let Some(hit) = ray.intersect_mesh(&draw.mesh, draw.model, cull(draw))?
                && hit.distance < nearest
            {
                nearest = hit.distance;
                selected = Some(Picked::Unit(*entity));
            }
        }
        if selected.is_some() {
            return Ok(selected);
        }
        Ok(screen.and_then(|screen| {
            self.near_miss(ray.origin(), screen, nearest.min(ray.max_distance()))
        }))
    }

    /// Presentation-only proxy boxes: each eligible target's world bounds, projected.
    fn near_miss(&self, origin: [f64; 3], screen: PickScreen, obstruction: f64) -> Option<Picked> {
        let mut proxies: Vec<(Picked, Aabb)> = Vec::new();
        let mut add = |target: Picked, draw: &SceneDraw| {
            let Some(bounds) = world_bounds(draw) else {
                return;
            };
            match proxies.iter_mut().find(|(t, _)| *t == target) {
                Some((_, b)) => b.union(&bounds),
                None => proxies.push((target, bounds)),
            }
        };
        for (door, draw) in self.world.iter().filter_map(|d| Some((door_of(d)?, d))) {
            add(Picked::Door(door), draw);
        }
        for (entity, draw) in &self.bodies {
            if matches!(entity, EntityRef::Creature(_) | EntityRef::Npc(_)) {
                add(Picked::Unit(*entity), draw);
            }
        }
        let [cursor_x, cursor_y] = screen.pixel;
        // Smallest miss, then nearest to the eye, then nearest centre on screen.
        let mut best: Option<(Picked, f64, f64, f64)> = None;
        for (target, bounds) in proxies {
            let centre = bounds.centre();
            let distance = (0..3)
                .map(|i| (centre[i] - origin[i]).powi(2))
                .sum::<f64>()
                .sqrt();
            if distance >= obstruction {
                continue;
            }
            let Some(rect) = bounds.screen_rect(screen.view_projection, screen.viewport) else {
                continue;
            };
            let Some([centre_x, centre_y]) =
                project(screen.view_projection, screen.viewport, centre)
            else {
                continue;
            };
            let dx = (rect[0] - cursor_x).max(cursor_x - rect[2]).max(0.);
            let dy = (rect[1] - cursor_y).max(cursor_y - rect[3]).max(0.);
            let miss = dx.hypot(dy);
            if miss > screen.margin {
                continue;
            }
            let on_screen = (centre_x - cursor_x).hypot(centre_y - cursor_y);
            let better = best.is_none_or(|(_, m, d, c)| {
                miss < m - 0.01
                    || ((miss - m).abs() <= 0.01
                        && (distance < d - 0.01 || ((distance - d).abs() <= 0.01 && on_screen < c)))
            });
            if better {
                best = Some((target, miss, distance, on_screen));
            }
        }
        best.map(|(target, ..)| target)
    }
}

/// The door whose leaf `draw` is: world draws of a walk-in building's door entry (#158).
fn door_of(draw: &SceneDraw) -> Option<DoorId> {
    mmorpg_presentation::buildings::BUILDINGS
        .iter()
        .find(|spec| draw.id.starts_with(spec.door_entry()))
        .map(|spec| spec.door)
}

fn cull(draw: &SceneDraw) -> CullMode {
    if draw.material.double_sided {
        CullMode::None
    } else {
        CullMode::Back
    }
}

#[derive(Clone, Copy)]
struct Aabb {
    min: [f64; 3],
    max: [f64; 3],
}
impl Aabb {
    fn union(&mut self, other: &Self) {
        for i in 0..3 {
            self.min[i] = self.min[i].min(other.min[i]);
            self.max[i] = self.max[i].max(other.max[i]);
        }
    }
    fn centre(&self) -> [f64; 3] {
        std::array::from_fn(|i| (self.min[i] + self.max[i]) * 0.5)
    }
    fn corners(&self) -> impl Iterator<Item = [f64; 3]> + '_ {
        (0..8).map(|c| {
            std::array::from_fn(|i| {
                if c >> i & 1 == 0 {
                    self.min[i]
                } else {
                    self.max[i]
                }
            })
        })
    }
    /// `[left, top, right, bottom]` in pixels; `None` if any corner is behind the eye.
    fn screen_rect(&self, view_projection: Mat4, viewport: [u32; 2]) -> Option<[f64; 4]> {
        let mut rect = [
            f64::INFINITY,
            f64::INFINITY,
            f64::NEG_INFINITY,
            f64::NEG_INFINITY,
        ];
        for corner in self.corners() {
            let [x, y] = project(view_projection, viewport, corner)?;
            rect = [
                rect[0].min(x),
                rect[1].min(y),
                rect[2].max(x),
                rect[3].max(y),
            ];
        }
        Some(rect)
    }
}

/// A draw's mesh bounds carried into the world by its model matrix.
fn world_bounds(draw: &SceneDraw) -> Option<Aabb> {
    let local = draw.mesh.bounds()?;
    let local = Aabb {
        min: [local.min.x, local.min.y, local.min.z].map(f64::from),
        max: [local.max.x, local.max.y, local.max.z].map(f64::from),
    };
    let m = draw.model.elements.map(f64::from);
    let mut world: Option<Aabb> = None;
    for [x, y, z] in local.corners() {
        let p: [f64; 3] =
            std::array::from_fn(|r| m[r] * x + m[4 + r] * y + m[8 + r] * z + m[12 + r]);
        let corner = Aabb { min: p, max: p };
        match world.as_mut() {
            Some(w) => w.union(&corner),
            None => world = Some(corner),
        }
    }
    world
}

/// Pixels from the top left, or `None` behind the eye.
fn project(view_projection: Mat4, viewport: [u32; 2], point: [f64; 3]) -> Option<[f64; 2]> {
    let m = view_projection.elements.map(f64::from);
    let clip: [f64; 4] = std::array::from_fn(|r| {
        m[r] * point[0] + m[4 + r] * point[1] + m[8 + r] * point[2] + m[12 + r]
    });
    if clip[3] <= 1e-4 {
        return None;
    }
    let [x, y] = [clip[0] / clip[3], clip[1] / clip[3]];
    Some([
        (x + 1.) * 0.5 * f64::from(viewport[0]),
        (1. - y) * 0.5 * f64::from(viewport[1]),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{camera::CameraView, world::DrawMaterial};
    use mmorpg_core::{CreatureId, NpcId};
    use std::sync::Arc;

    const VIEWPORT: [u32; 2] = [800, 600];

    fn view() -> CameraView {
        CameraView {
            field_of_view_degrees: 60.,
            eye: [0., 1.5, -8.],
            target: [0., 1., 0.],
        }
    }

    /// An axis-aligned box draw centred at `at` with `size`.
    fn cube(id: &str, at: [f32; 3], size: [f32; 3]) -> SceneDraw {
        let mut elements = [0.; 16];
        elements[0] = size[0];
        elements[5] = size[1];
        elements[10] = size[2];
        elements[12..15].copy_from_slice(&at);
        elements[15] = 1.;
        SceneDraw {
            id: id.into(),
            resource_key: "cube".into(),
            mesh: Arc::new(three_d_core::Mesh::unit_cube()),
            model: Mat4 { elements },
            material: DrawMaterial {
                color: [1.; 3],
                emissive: [0.; 3],
                opacity: 1.,
                unlit: false,
                double_sided: true,
            },
        }
    }

    fn picking(world: Vec<SceneDraw>, bodies: Vec<(EntityRef, SceneDraw)>) -> WorldPicking {
        WorldPicking { world, bodies }
    }

    fn pixel_of(point: [f32; 3]) -> [f64; 2] {
        crate::graphics::project_to_viewport(view(), VIEWPORT, point)
            .expect("on screen")
            .map(f64::from)
    }

    fn click(p: &WorldPicking, pixel: [f64; 2]) -> Option<Picked> {
        let view_projection = crate::graphics::scene_camera(view(), VIEWPORT)
            .unwrap()
            .view_projection_matrix();
        let ray = PickRay::from_viewport(view_projection, VIEWPORT, pixel).unwrap();
        let screen = PickScreen {
            view_projection,
            viewport: VIEWPORT,
            pixel,
            margin: NEAR_MISS_PIXELS,
        };
        p.select_frame(ray, Some(screen)).unwrap()
    }

    const WOLF: EntityRef = EntityRef::Creature(CreatureId::new(7));

    /// A wolf torso at the origin and ground far behind it.
    fn wolf_scene() -> WorldPicking {
        picking(
            vec![cube("ground", [0., -0.5, 20.], [40., 1., 1.])],
            vec![(
                WOLF,
                cube("unit-creature-7-body", [0., 0.5, 0.], [0.6, 0.6, 1.]),
            )],
        )
    }

    #[test]
    fn exact_hits_and_near_misses_select_the_creature() {
        let p = wolf_scene();
        assert_eq!(click(&p, pixel_of([0., 0.5, 0.])), Some(Picked::Unit(WOLF)));
        // Just outside the torso's projected side edge.
        let [x, y] = pixel_of([0.3, 0.5, -0.5]);
        let out = (x - pixel_of([0., 0.5, -0.5])[0]).signum();
        assert_eq!(click(&p, [x + 5. * out, y]), Some(Picked::Unit(WOLF)));
        assert_eq!(
            click(&p, [x + 30. * out, y]),
            None,
            "beyond the margin stays empty"
        );
    }

    #[test]
    fn walls_in_front_still_block_near_misses() {
        let mut p = wolf_scene();
        p.world.push(cube("wall", [0.5, 1., -2.], [1., 2., 0.2]));
        let [x, y] = pixel_of([0.3, 0.5, -0.5]);
        let out = (x - pixel_of([0., 0.5, -0.5])[0]).signum();
        assert_eq!(click(&p, [x + 5. * out, y]), None);
    }

    #[test]
    fn players_are_not_assisted_and_the_closer_miss_wins() {
        let left = EntityRef::Npc(NpcId::new(1));
        let right = EntityRef::Npc(NpcId::new(2));
        let player = EntityRef::Player(3);
        let p = picking(
            Vec::new(),
            vec![
                (
                    left,
                    cube("unit-npc-1-body", [-0.6, 1., 0.], [0.5, 1.8, 0.4]),
                ),
                (
                    right,
                    cube("unit-npc-2-body", [0.6, 1., 0.], [0.5, 1.8, 0.4]),
                ),
                (
                    player,
                    cube("unit-player-3-body", [0., 1., 3.], [0.5, 1.8, 0.4]),
                ),
            ],
        );
        // In the gap between their inner edges, each click closer to one of them.
        let [lx, y] = pixel_of([-0.35, 1., -0.2]);
        let [rx, _] = pixel_of([0.35, 1., -0.2]);
        let towards_right = (rx - lx).signum();
        assert_eq!(
            click(&p, [rx - 3. * towards_right, y]),
            Some(Picked::Unit(right))
        );
        assert_eq!(
            click(&p, [lx + 3. * towards_right, y]),
            Some(Picked::Unit(left))
        );
        // Above the player's head only: no assisted player selection.
        let [px, py] = pixel_of([0., 1.9, 3.]);
        assert_eq!(click(&p, [px, py - 5.]), None);
    }

    #[test]
    fn the_door_leaf_is_forgiving_too() {
        let p = picking(
            vec![cube("greyhaven.inn.door.leaf", [0., 1., 0.], [1., 2., 0.1])],
            Vec::new(),
        );
        let [x, y] = pixel_of([0.5, 1., -0.05]);
        let out = (x - pixel_of([0., 1., -0.05])[0]).signum();
        assert_eq!(
            click(&p, [x + 4. * out, y]),
            Some(Picked::Door(mmorpg_presentation::buildings::INN.door))
        );
    }
}
