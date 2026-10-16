//! Entity identity over the exact geometry of the successfully presented native frame.
use crate::{
    ClientError,
    world::{SceneDraw, WorldFrame},
};
use mmorpg_core::EntityRef;
use three_d_projective::picking::{CullMode, PickRay};

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
    pub fn select_frame(&self, ray: PickRay) -> Result<Option<EntityRef>, ClientError> {
        let mut nearest = f64::INFINITY;
        let mut selected = None;
        for draw in &self.world {
            if let Some(hit) = ray.intersect_mesh(&draw.mesh, draw.model, cull(draw))? {
                nearest = nearest.min(hit.distance);
            }
        }
        for (entity, draw) in &self.bodies {
            if let Some(hit) = ray.intersect_mesh(&draw.mesh, draw.model, cull(draw))?
                && hit.distance < nearest
            {
                nearest = hit.distance;
                selected = Some(*entity);
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
