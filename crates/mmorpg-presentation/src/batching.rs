//! Spatial batching and immutable resources for Greyhaven's procedural world.
use super::geometry::*;
use super::{Draw, Group, Kit, Material};
use std::{collections::BTreeMap, sync::Arc};
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
/// Distance class of a static group.
pub enum Cull {
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
    /// Camera distance (metres, to the group's bounding circle) beyond which the class is hidden.
    #[must_use]
    pub fn reach(self) -> f64 {
        match self {
            Self::Always => f64::INFINITY,
            Self::Far => 420.,
            Self::Mid => 150.,
            Self::Near => 75.,
        }
    }
}
/// `(key, cull, fallback kit, geometry index, per-instance transform and color)`.
type InstanceBatch = (String, Cull, Option<Kit>, usize, Vec<(Affine, u32)>);
#[derive(Default)]
pub struct Batcher {
    /// The kit whose fallback the following pieces are (#49): they batch apart from the rest.
    pub fallback: Option<Kit>,
    buckets: BTreeMap<String, (u32, Cull, Option<Kit>, Geometry)>,
    geometries: Vec<Arc<Geometry>>,
    instances: Vec<InstanceBatch>,
    /// Each instance batch's index in `instances`, by key.
    instance_keys: BTreeMap<String, usize>,
}
pub fn rgb(c: u32) -> [f32; 3] {
    [16, 8, 0].map(|shift| ((c >> shift) & 255) as f32 / 255.)
}
pub fn shade(c: u32, f: f64) -> u32 {
    [16, 8, 0].into_iter().fold(0, |out, s| {
        out | ((((f64::from((c >> s) & 255) * f).clamp(0., 255.) + 0.5).floor() as u32) << s)
    })
}
pub fn mix(a: u32, b: u32, t: f64) -> u32 {
    [16, 8, 0].into_iter().fold(0, |out, s| {
        let x = f64::from((a >> s) & 255);
        let y = f64::from((b >> s) & 255);
        out | (((x + (y - x) * t.clamp(0., 1.)).round() as u32) << s)
    })
}
pub fn matrix(a: Affine) -> [f32; 16] {
    let m = a.m;
    {
        [
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
        ]
    }
}
pub fn material(c: u32, glow: bool) -> Material {
    Material {
        color: rgb(c),
        emissive: if glow { rgb(c) } else { [0.; 3] },
        opacity: 1.,
        unlit: false,
        double_sided: false,
    }
}
pub fn emissive(c: u32) -> bool {
    [0x7fe6ff, 0xffd98a, 0xf2c46a, 0xff8a2a, 0xffe27a].contains(&c)
}
impl Batcher {
    pub fn global(&mut self, name: &str, c: u32) -> &mut Geometry {
        self.bucket(format!("all-{name}"), c, Cull::Always)
    }
    fn bucket(&mut self, chunk: String, c: u32, cull: Cull) -> &mut Geometry {
        let kit = self.fallback;
        &mut self
            .buckets
            .entry(format!(
                "{chunk}:{}{}:{c:06x}",
                cull.name(),
                fallback_name(kit)
            ))
            .or_insert_with(|| (c, cull, kit, Geometry::default()))
            .3
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
        self.instance("scenery", c, s, a, [x, z], cull);
    }
    /// One instance of the shared geometry `s` placed by `a`, batched by `name`, distance
    /// class and spatial chunk (around `at`, x and z in metres). Every batch of the same
    /// geometry shares one vertex buffer, so many small repeated pieces cost one mesh.
    pub fn instance(
        &mut self,
        name: &str,
        c: u32,
        s: &Geometry,
        a: Affine,
        at: [f64; 2],
        cull: Cull,
    ) {
        let index = if let Some(i) = self.geometries.iter().position(|g| **g == *s) {
            i
        } else {
            let i = self.geometries.len();
            self.geometries.push(Arc::new(s.clone()));
            i
        };
        let size = cull.size() * 2.;
        let kit = self.fallback;
        let key = format!(
            "{size}m{}_{}:{}{}:{name}:instance-geometry:{index}",
            (at[0] / size).floor(),
            (at[1] / size).floor(),
            cull.name(),
            fallback_name(kit)
        );
        let instances = &mut self.instances;
        let i = *self.instance_keys.entry(key).or_insert_with_key(|key| {
            instances.push((key.clone(), cull, kit, index, Vec::new()));
            instances.len() - 1
        });
        self.instances[i].4.push((a, c));
    }
    pub fn finish(self) -> Vec<Group> {
        let mut groups = Vec::new();
        for (name, (c, cull, fallback, g)) in self.buckets {
            if g.indices.is_empty() {
                continue;
            }
            let (min, max) = bounds(g.p.iter().copied());
            let id = format!("static-{name}");
            let draw = Draw {
                id,
                resource_key: format!("scenery:{name}"),
                geometry: Arc::new(g),
                model: matrix(ID),
                material: material(c, emissive(c)),
            };
            groups.push(Group {
                draws: vec![draw],
                cull,
                instanced: false,
                fallback,
                min,
                max,
            });
        }
        for (key, cull, fallback, index, instances) in self.instances {
            let g = &self.geometries[index];
            let (min, max) = bounds(
                instances
                    .iter()
                    .flat_map(|(a, _)| g.p.iter().map(move |p| point(*a, *p))),
            );
            let draws = instances
                .into_iter()
                .enumerate()
                .map(|(i, (a, c))| Draw {
                    id: format!("instances:{key}:{i}"),
                    resource_key: format!("scenery:instance-geometry:{index}"),
                    geometry: Arc::clone(g),
                    model: matrix(a),
                    material: material(c, false),
                })
                .collect();
            groups.push(Group {
                draws,
                cull,
                instanced: true,
                fallback,
                min,
                max,
            });
        }
        groups
    }
}
/// A fallback kit's suffix in batch keys.
fn fallback_name(kit: Option<Kit>) -> &'static str {
    match kit {
        None => "",
        Some(Kit::Outpost) => ":fallback-outpost",
        Some(Kit::SettlementProps) => ":fallback-settlement",
        Some(Kit::Nature) => ":fallback-nature",
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
