//! Entity identity over the exact geometry of the successfully presented native frame.
use crate::{
    ClientError,
    world::{SceneDraw, WorldFrame},
};
use mmorpg_core::{DoorId, EntityRef};
use three_d_projective::picking::{CullMode, PickRay};

/// What a click lands on first.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Picked {
    Unit(EntityRef),
    Door(DoorId),
}

/// World draws with this id prefix are the inn's door leaf (#158).
pub const DOOR_DRAW_PREFIX: &str = "greyhaven.inn.door";

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
    pub fn select_frame(&self, ray: PickRay) -> Result<Option<Picked>, ClientError> {
        let mut nearest = f64::INFINITY;
        let mut selected = None;
        for draw in &self.world {
            if let Some(hit) = ray.intersect_mesh(&draw.mesh, draw.model, cull(draw))?
                && hit.distance < nearest
            {
                nearest = hit.distance;
                selected = draw
                    .id
                    .starts_with(DOOR_DRAW_PREFIX)
                    .then_some(Picked::Door(mmorpg_presentation::buildings::INN_DOOR));
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
        Ok(selected)
    }
}
fn cull(draw: &SceneDraw) -> CullMode {
    if draw.material.double_sided {
        CullMode::None
    } else {
        CullMode::Back
    }
}
