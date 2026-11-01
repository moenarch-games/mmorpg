//! Shared procedural shapes (`mmorpg-presentation`) and their native mesh lowering.
pub(crate) use mmorpg_presentation::geometry::*;
use three_d_core::{Color3, Mesh, Tangent4, Vec2, Vec3, VertexAttributes};

/// Lowers shared geometry to an immutable 3d-lab mesh.
pub(crate) trait ToMesh {
    fn mesh(&self) -> Mesh;
}

impl ToMesh for Geometry {
    fn mesh(&self) -> Mesh {
        let vec = |p: &V| Vec3::new(p[0] as f32, p[1] as f32, p[2] as f32);
        Mesh::with_attributes(
            self.p.iter().map(vec).collect(),
            self.indices.clone(),
            VertexAttributes {
                normals: Some(self.n.iter().map(vec).collect()),
                colors: (!self.colors.is_empty()).then(|| {
                    self.colors
                        .iter()
                        .map(|c| Color3::new(c[0] as f32, c[1] as f32, c[2] as f32))
                        .collect()
                }),
                // Ground layers (#525) are textured.
                uvs: (!self.uv.is_empty()).then(|| {
                    self.uv
                        .iter()
                        .map(|[u, v]| Vec2::new(*u as f32, *v as f32))
                        .collect()
                }),
                tangents: (!self.tangents.is_empty()).then(|| {
                    self.tangents
                        .iter()
                        .map(|t| Tangent4::new(t[0] as f32, t[1] as f32, t[2] as f32, t[3] as f32))
                        .collect()
                }),
            },
        )
        .expect("procedural shapes contain finite vertices and valid indices")
    }
}
