//! Simplified authored collision, in integer centimetres, independent of render meshes.
use super::{StaticCollider, within_content_range};
use crate::ZoneError;

/// One fixed triangle mesh. Vertices are local to `position`; winding points out of solids.
/// Stable IDs share the static collider namespace. JSON is an authoring input, never a wire format.
#[derive(Clone, Debug, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub struct StaticMesh {
    pub id: u32,
    pub position: [i32; 3],
    pub vertices: Vec<[i32; 3]>,
    pub triangles: Vec<[u32; 3]>,
}

impl StaticMesh {
    pub(crate) fn bounds(&self) -> Result<StaticCollider, ZoneError> {
        if self.vertices.len() < 3 || self.triangles.is_empty() {
            return Err(ZoneError::new("static mesh needs vertices and triangles"));
        }
        let mut min = [i32::MAX; 3];
        let mut max = [i32::MIN; 3];
        for vertex in &self.vertices {
            for axis in 0..3 {
                let world = self.position[axis]
                    .checked_add(vertex[axis])
                    .filter(|v| within_content_range(*v))
                    .ok_or_else(|| ZoneError::new("static mesh vertex outside content range"))?;
                min[axis] = min[axis].min(world);
                max[axis] = max[axis].max(world);
            }
        }
        for triangle in &self.triangles {
            let points: Vec<_> = triangle
                .iter()
                .map(|i| self.vertices.get(*i as usize))
                .collect::<Option<_>>()
                .ok_or_else(|| ZoneError::new("static mesh triangle index outside vertices"))?;
            let a: [i64; 3] =
                std::array::from_fn(|i| i64::from(points[1][i]) - i64::from(points[0][i]));
            let b: [i64; 3] =
                std::array::from_fn(|i| i64::from(points[2][i]) - i64::from(points[0][i]));
            if [
                a[1] * b[2] - a[2] * b[1],
                a[2] * b[0] - a[0] * b[2],
                a[0] * b[1] - a[1] * b[0],
            ] == [0; 3]
            {
                return Err(ZoneError::new("static mesh triangle is degenerate"));
            }
        }
        // Conservative integer bounds, including planar surfaces and odd spans.
        let center = std::array::from_fn(|i| (min[i] + max[i]) / 2);
        let half_extents =
            std::array::from_fn(|i| (max[i] - center[i]).max(center[i] - min[i]).max(1));
        Ok(StaticCollider {
            id: self.id,
            position: center,
            half_extents,
        })
    }
}
