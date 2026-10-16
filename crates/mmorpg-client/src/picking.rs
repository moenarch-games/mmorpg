//! Presentation identity composition over the pinned renderer's geometry queries.
use crate::{ClientError, presentation::SceneBox, world::WorldScene};
use mmorpg_core::EntityRef;
use three_d_animation::Mat4;
use three_d_core::{Mesh, Vec3};
use three_d_projective::picking::{CullMode, PickRay};

pub struct WorldPicking {
    surfaces: Vec<Mesh>,
    props: Vec<SceneBox>,
    cube: Mesh,
}
impl WorldPicking {
    pub fn new(world: &WorldScene) -> Result<Self, ClientError> {
        let surfaces = [&world.terrain, &world.water]
            .into_iter()
            .filter(|m| !m.indices.is_empty())
            .map(|mesh| {
                Mesh::new(
                    mesh.vertices
                        .iter()
                        .map(|v| Vec3::new(v.position[0], v.position[1], v.position[2]))
                        .collect(),
                    mesh.indices.clone(),
                )
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Self {
            surfaces,
            props: world.props.clone(),
            cube: Mesh::unit_cube(),
        })
    }
    pub fn select(
        &self,
        ray: PickRay,
        bodies: &[(EntityRef, SceneBox)],
    ) -> Result<Option<EntityRef>, ClientError> {
        let mut nearest = f64::INFINITY;
        let mut selected = None;
        for mesh in &self.surfaces {
            if let Some(hit) = ray.intersect_mesh(mesh, Mat4::IDENTITY, CullMode::Back)? {
                nearest = nearest.min(hit.distance);
            }
        }
        for part in &self.props {
            if let Some(hit) =
                ray.intersect_mesh(&self.cube, box_transform(*part), CullMode::Back)?
            {
                nearest = nearest.min(hit.distance);
            }
        }
        for (entity, part) in bodies {
            if let Some(hit) =
                ray.intersect_mesh(&self.cube, box_transform(*part), CullMode::Back)?
                && hit.distance < nearest
            {
                nearest = hit.distance;
                selected = Some(*entity);
            }
        }
        Ok(selected)
    }
}
fn box_transform(part: SceneBox) -> Mat4 {
    // Same scale, yaw and translation as scene.wgsl; the foundation owns intersection math.
    let (s, c) = part.yaw.sin_cos();
    let [x, y, z] = part.size;
    let [tx, ty, tz] = part.position;
    Mat4 {
        elements: [
            c * x,
            0.0,
            -s * x,
            0.0,
            0.0,
            y,
            0.0,
            0.0,
            s * z,
            0.0,
            c * z,
            0.0,
            tx,
            ty,
            tz,
            1.0,
        ],
    }
}
