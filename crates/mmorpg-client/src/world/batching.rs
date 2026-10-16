//! Spatial batching and immutable resources for Greyhaven's procedural world.
use super::geometry::*;
use super::{DrawMaterial, SceneDraw};
use std::{collections::BTreeMap, sync::Arc};
use three_d_animation::Mat4;
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum Cull {
    Always,
    Far,
    Mid,
    Near,
}
impl Cull {
    pub fn name(self) -> &'static str {
        match self {
            Self::Always => "always",
            Self::Far => "far",
            Self::Mid => "mid",
            Self::Near => "near",
        }
    }
    pub fn size(self) -> f64 {
        match self {
            Self::Always => 120.,
            Self::Far => 80.,
            Self::Mid => 60.,
            Self::Near => 40.,
        }
    }
    fn reach(self) -> f64 {
        match self {
            Self::Always => f64::INFINITY,
            Self::Far => 420.,
            Self::Mid => 150.,
            Self::Near => 75.,
        }
    }
}
#[derive(Clone, Debug)]
pub(super) struct Group {
    pub draws: Vec<SceneDraw>,
    pub cull: Cull,
    pub min: [f64; 2],
    pub max: [f64; 2],
}
impl Group {
    pub fn visible(&self, eye: [f32; 3]) -> bool {
        let c = [
            (self.min[0] + self.max[0]) / 2.,
            (self.min[1] + self.max[1]) / 2.,
        ];
        let r = (self.max[0] - self.min[0]).hypot(self.max[1] - self.min[1]) / 2.;
        (c[0] - f64::from(eye[0])).hypot(c[1] - f64::from(eye[2])) - r <= self.cull.reach()
    }
}
#[derive(Default)]
pub(super) struct Batcher {
    buckets: BTreeMap<String, (u32, Cull, Geometry)>,
    geometries: Vec<(Geometry, Arc<three_d_core::Mesh>)>,
    instances: Vec<(String, Cull, usize, Vec<(Affine, u32)>)>,
}
pub(super) fn rgb(c: u32) -> [f32; 3] {
    [16, 8, 0].map(|shift| ((c >> shift) & 255) as f32 / 255.)
}
pub(super) fn shade(c: u32, f: f64) -> u32 {
    [16, 8, 0].into_iter().fold(0, |out, s| {
        out | ((((f64::from((c >> s) & 255) * f).clamp(0., 255.) + 0.5).floor() as u32) << s)
    })
}
pub(super) fn mix(a: u32, b: u32, t: f64) -> u32 {
    [16, 8, 0].into_iter().fold(0, |out, s| {
        let x = f64::from((a >> s) & 255);
        let y = f64::from((b >> s) & 255);
        out | (((x + (y - x) * t.clamp(0., 1.)).round() as u32) << s)
    })
}
pub(super) fn matrix(a: Affine) -> Mat4 {
    let m = a.m;
    Mat4 {
        elements: [
            m[0] as f32,
            m[3] as f32,
            m[6] as f32,
            0.,
            m[1] as f32,
            m[4] as f32,
            m[7] as f32,
            0.,
            m[2] as f32,
            m[5] as f32,
            m[8] as f32,
            0.,
            a.t[0] as f32,
            a.t[1] as f32,
            a.t[2] as f32,
            1.,
        ],
    }
}
pub(super) fn material(c: u32, glow: bool) -> DrawMaterial {
    DrawMaterial {
        color: rgb(c),
        emissive: if glow { rgb(c) } else { [0.; 3] },
        opacity: 1.,
        unlit: false,
        double_sided: false,
    }
}
pub(super) fn emissive(c: u32) -> bool {
    [0x7fe6ff, 0xffd98a, 0xf2c46a, 0xff8a2a, 0xffe27a].contains(&c)
}
impl Batcher {
    pub fn global(&mut self, name: &str, c: u32) -> &mut Geometry {
        self.bucket(format!("all-{name}"), c, Cull::Always)
    }
    fn bucket(&mut self, chunk: String, c: u32, cull: Cull) -> &mut Geometry {
        &mut self
            .buckets
            .entry(format!("{chunk}:{}:{c:06x}", cull.name()))
            .or_insert_with(|| (c, cull, Geometry::default()))
            .2
    }
    pub fn put(
        &mut self,
        c: u32,
        s: &Geometry,
        a: Affine,
        x: f64,
        z: f64,
        cull: Cull,
        instance: bool,
    ) {
        if !instance {
            let size = cull.size();
            let chunk = format!("{size}m{}_{}", (x / size).floor(), (z / size).floor());
            self.bucket(chunk, c, cull).add(s, a);
            return;
        }
        let index = if let Some(i) = self.geometries.iter().position(|(g, _)| g == s) {
            i
        } else {
            let i = self.geometries.len();
            self.geometries.push((s.clone(), Arc::new(s.mesh())));
            i
        };
        let size = cull.size() * 2.;
        let key = format!(
            "{size}m{}_{}:{}:native-scenery:instance-geometry:{index}",
            (x / size).floor(),
            (z / size).floor(),
            cull.name()
        );
        let i = if let Some(i) = self.instances.iter().position(|(k, ..)| k == &key) {
            i
        } else {
            let i = self.instances.len();
            self.instances.push((key, cull, index, Vec::new()));
            i
        };
        self.instances[i].3.push((a, c));
    }
    pub fn finish(self) -> Vec<Group> {
        let mut groups = Vec::new();
        for (name, (c, cull, g)) in self.buckets {
            if g.indices.is_empty() {
                continue;
            }
            let (min, max) = bounds(g.p.iter().copied());
            let id = format!("static-{name}");
            let draw = SceneDraw {
                id,
                resource_key: format!("native-scenery:{name}"),
                mesh: Arc::new(g.mesh()),
                model: matrix(ID),
                material: material(c, emissive(c)),
            };
            groups.push(Group {
                draws: vec![draw],
                cull,
                min,
                max,
            });
        }
        for (key, cull, index, instances) in self.instances {
            let (g, mesh) = &self.geometries[index];
            let (min, max) = bounds(
                instances
                    .iter()
                    .flat_map(|(a, _)| g.p.iter().map(move |p| point(*a, *p))),
            );
            let draws = instances
                .into_iter()
                .enumerate()
                .map(|(i, (a, c))| SceneDraw {
                    id: format!("instances:{key}:{i}"),
                    resource_key: format!("native-scenery:instance-geometry:{index}"),
                    mesh: Arc::clone(mesh),
                    model: matrix(a),
                    material: material(c, false),
                })
                .collect();
            groups.push(Group {
                draws,
                cull,
                min,
                max,
            });
        }
        groups
    }
}
fn bounds(points: impl Iterator<Item = V>) -> ([f64; 2], [f64; 2]) {
    points.fold(
        ([f64::INFINITY; 2], [f64::NEG_INFINITY; 2]),
        |(mut lo, mut hi), p| {
            for (i, j) in [0, 2].into_iter().enumerate() {
                lo[i] = lo[i].min(p[j]);
                hi[i] = hi[i].max(p[j]);
            }
            (lo, hi)
        },
    )
}
