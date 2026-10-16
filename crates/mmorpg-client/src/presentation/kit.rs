//! Unit joint frames and shared geometry; contains no gameplay or GPU state.
use crate::world::{DrawMaterial, SceneDraw, geometry::*};
use std::{
    collections::BTreeMap,
    f64::consts::{PI, TAU},
    sync::Arc,
};
use three_d_animation::Mat4;
pub(super) type Q = [f64; 4];
pub(super) const IQ: Q = [0., 0., 0., 1.];
#[derive(Clone, Copy)]
pub(super) struct Frame {
    pub p: V,
    pub q: Q,
}
pub(super) fn qa(axis: usize, a: f64) -> Q {
    let mut q = [0., 0., 0., (a / 2.).cos()];
    q[axis] = (a / 2.).sin();
    q
}
pub(super) fn qm(a: Q, b: Q) -> Q {
    let [x, y, z, w] = a;
    let [u, v, t, s] = b;
    [
        w * u + x * s + y * t - z * v,
        w * v - x * t + y * s + z * u,
        w * t + x * v - y * u + z * s,
        w * s - x * u - y * v - z * t,
    ]
}
pub(super) fn qr(q: Q, v: V) -> V {
    let [x, y, z, w] = q;
    let [a, b, c] = v;
    let (cx, cy, cz) = (y * c - z * b, z * a - x * c, x * b - y * a);
    [
        a + 2. * (w * cx + y * cz - z * cy),
        b + 2. * (w * cy + z * cx - x * cz),
        c + 2. * (w * cz + x * cy - y * cx),
    ]
}
pub(super) fn child(f: Frame, p: V, q: Q) -> Frame {
    let d = qr(f.q, p);
    Frame {
        p: std::array::from_fn(|i| f.p[i] + d[i]),
        q: qm(f.q, q),
    }
}
pub(super) fn offset(f: Frame, p: V) -> Frame {
    child(f, p, IQ)
}
pub(super) fn tilt(pitch: f64, roll: f64, yaw: f64) -> Q {
    qm(qm(qa(1, yaw), qa(0, pitch)), qa(2, roll))
}
pub(super) fn model(f: Frame, s: V) -> Mat4 {
    let len = f.q.iter().map(|v| v * v).sum::<f64>().sqrt();
    let [x, y, z, w] = f.q.map(|v| v / len);
    let a = Affine {
        m: [
            1. - 2. * (y * y + z * z),
            2. * (x * y - z * w),
            2. * (x * z + y * w),
            2. * (x * y + z * w),
            1. - 2. * (x * x + z * z),
            2. * (y * z - x * w),
            2. * (x * z - y * w),
            2. * (y * z + x * w),
            1. - 2. * (x * x + y * y),
        ],
        t: f.p,
    };
    let a = compose(&[a, scale(s[0], s[1], s[2])]);
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
pub(super) fn rgb(c: u32) -> [f32; 3] {
    [16, 8, 0].map(|s| ((c >> s) & 255) as f32 / 255.)
}
pub(super) fn shade(c: u32, f: f64) -> u32 {
    [16, 8, 0].into_iter().fold(0, |o, s| {
        o | (((f64::from((c >> s) & 255) * f).round().clamp(0., 255.) as u32) << s)
    })
}
pub(super) fn mix(c: u32, d: u32, t: f64) -> u32 {
    [16, 8, 0].into_iter().fold(0, |o, s| {
        let a = f64::from((c >> s) & 255);
        let b = f64::from((d >> s) & 255);
        o | (((a + (b - a) * t).round() as u32) << s)
    })
}
#[derive(Default)]
pub(super) struct Resources {
    meshes: BTreeMap<String, Arc<three_d_core::Mesh>>,
}
impl Resources {
    pub fn mesh(&mut self, key: &str, build: impl FnOnce() -> Geometry) -> Arc<three_d_core::Mesh> {
        Arc::clone(
            self.meshes
                .entry(key.into())
                .or_insert_with(|| Arc::new(build().mesh())),
        )
    }
    pub fn draw(&mut self, id: String, shape: &str, c: u32, f: Frame, s: V) -> SceneDraw {
        let key = format!("humanoid:{shape}");
        let mesh = self.mesh(&key, || match shape {
            "limb" => {
                let mut g = Geometry::default();
                g.add(&frustum(8, 0.78, true, true, 0.), rx(PI));
                g
            }
            "skirt" => {
                let mut g = Geometry::default();
                g.add(&frustum(10, 1.45, true, false, 0.), rx(PI));
                g
            }
            "post" => frustum(8, 1., true, true, 0.),
            "blob" => blob(1, true, 0., 0.),
            "box" => box_shape(),
            "cone" => frustum(8, 0., true, true, 0.),
            "disc" => frustum(14, 1., false, true, 0.),
            _ => frustum(
                4,
                shape
                    .strip_prefix("torso-")
                    .and_then(|s| s.parse::<f64>().ok())
                    .unwrap_or(100.)
                    / 100.,
                false,
                true,
                0.5,
            ),
        });
        SceneDraw {
            id,
            resource_key: key,
            mesh,
            model: model(f, s),
            material: DrawMaterial {
                color: rgb(c),
                emissive: [0.; 3],
                opacity: 1.,
                unlit: false,
                double_sided: false,
            },
        }
    }
    pub fn primitive(&mut self, id: String, shape: Primitive, c: u32, f: Frame) -> SceneDraw {
        let key = format!("unit-primitive:{shape:?}");
        let mesh = self.mesh(&key, || match shape {
            Primitive::Box(size) => {
                let mut g = Geometry::default();
                g.add(
                    &box_shape(),
                    scale(size[0] / 2., size[1] / 2., size[2] / 2.),
                );
                g
            }
            Primitive::Ball(r) => sphere(r),
            Primitive::Post(r, h) => {
                let mut g = Geometry::default();
                g.add(
                    &frustum(20, 1., true, true, 0.),
                    compose(&[translate(0., -h / 2., 0.), scale(r, h, r)]),
                );
                g
            }
        });
        SceneDraw {
            id,
            resource_key: key,
            mesh,
            model: model(f, [1.; 3]),
            material: DrawMaterial {
                color: rgb(c),
                emissive: [0.; 3],
                opacity: 1.,
                unlit: false,
                double_sided: false,
            },
        }
    }
}
#[derive(Clone, Copy, Debug)]
pub(super) enum Primitive {
    Box(V),
    Ball(f64),
    Post(f64, f64),
}
fn sphere(r: f64) -> Geometry {
    let mut g = Geometry::default();
    for row in 0..=12 {
        let phi = row as f64 / 12. * PI;
        for col in 0..=20 {
            let theta = col as f64 / 20. * TAU;
            let n = [-theta.cos() * phi.sin(), phi.cos(), theta.sin() * phi.sin()];
            g.p.push(n.map(|v| v * r));
            g.n.push(n);
        }
    }
    for row in 0..12 {
        for col in 0..20 {
            let a = row * 21 + col + 1;
            let b = a - 1;
            let c = b + 21;
            let d = c + 1;
            if row != 0 {
                g.indices.extend([a, b, d]);
            }
            if row != 11 {
                g.indices.extend([b, c, d]);
            }
        }
    }
    g
}
pub(super) fn root(p: V, yaw: f64, pivot: f64, lay: Option<(Q, f64)>) -> Frame {
    let base = Frame { p, q: qa(1, yaw) };
    if let Some((q, lift)) = lay {
        let [x, y, z] = qr(q, [0., pivot, 0.]);
        child(base, [-x, lift - y, -z], q)
    } else {
        base
    }
}
