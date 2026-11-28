//! Unit joint frames and shared geometry; contains no gameplay or GPU state.
use crate::{Draw, Material, colour::rgb, geometry::*};
use std::{
    collections::BTreeMap,
    f64::consts::{PI, TAU},
    sync::Arc,
};
pub type Q = [f64; 4];
pub const IQ: Q = [0., 0., 0., 1.];
#[derive(Clone, Copy, Debug)]
pub struct Frame {
    pub p: V,
    pub q: Q,
}
pub fn qa(axis: usize, a: f64) -> Q {
    let mut q = [0., 0., 0., (a / 2.).cos()];
    q[axis] = (a / 2.).sin();
    q
}
/// The quaternion product `a × b` (`b` applied first) in `[x, y, z, w]` order, in either
/// precision: the native skinning composes its single-precision joints with it too.
pub fn qm<T: Real>(a: [T; 4], b: [T; 4]) -> [T; 4] {
    let [x, y, z, w] = a;
    let [u, v, t, s] = b;
    [
        w * u + x * s + y * t - z * v,
        w * v - x * t + y * s + z * u,
        w * t + x * v - y * u + z * s,
        w * s - x * u - y * v - z * t,
    ]
}
/// The rotation taking the unit axes X, Y and Z onto the orthonormal `x`, `y` and `z` (a
/// rotation matrix's columns), as a quaternion.
pub fn from_axes<T: Real>(x: [T; 3], y: [T; 3], z: [T; 3]) -> [T; 4] {
    let (one, two, quarter) = (T::from(1.), T::from(2.), T::from(0.25));
    let trace = x[0] + y[1] + z[2];
    if trace > T::from(0.) {
        let s = (trace + one).sqrt() * two;
        [
            (y[2] - z[1]) / s,
            (z[0] - x[2]) / s,
            (x[1] - y[0]) / s,
            quarter * s,
        ]
    } else if x[0] > y[1] && x[0] > z[2] {
        let s = (one + x[0] - y[1] - z[2]).sqrt() * two;
        [
            quarter * s,
            (y[0] + x[1]) / s,
            (z[0] + x[2]) / s,
            (y[2] - z[1]) / s,
        ]
    } else if y[1] > z[2] {
        let s = (one + y[1] - x[0] - z[2]).sqrt() * two;
        [
            (y[0] + x[1]) / s,
            quarter * s,
            (z[1] + y[2]) / s,
            (z[0] - x[2]) / s,
        ]
    } else {
        let s = (one + z[2] - x[0] - y[1]).sqrt() * two;
        [
            (z[0] + x[2]) / s,
            (z[1] + y[2]) / s,
            quarter * s,
            (x[1] - y[0]) / s,
        ]
    }
}
/// The float types the quaternion helpers take: the units' double and the native skinning's
/// single precision.
pub trait Real:
    Copy
    + PartialOrd
    + From<f32>
    + std::ops::Add<Output = Self>
    + std::ops::Sub<Output = Self>
    + std::ops::Mul<Output = Self>
    + std::ops::Div<Output = Self>
{
    #[must_use]
    fn sqrt(self) -> Self;
}
impl Real for f32 {
    fn sqrt(self) -> Self {
        f32::sqrt(self)
    }
}
impl Real for f64 {
    fn sqrt(self) -> Self {
        f64::sqrt(self)
    }
}
pub fn qr(q: Q, v: V) -> V {
    let [x, y, z, w] = q;
    let [a, b, c] = v;
    let (cx, cy, cz) = (y * c - z * b, z * a - x * c, x * b - y * a);
    [
        a + 2. * (w * cx + y * cz - z * cy),
        b + 2. * (w * cy + z * cx - x * cz),
        c + 2. * (w * cz + x * cy - y * cx),
    ]
}
pub fn child(f: Frame, p: V, q: Q) -> Frame {
    let d = qr(f.q, p);
    Frame {
        p: std::array::from_fn(|i| f.p[i] + d[i]),
        q: qm(f.q, q),
    }
}
pub fn offset(f: Frame, p: V) -> Frame {
    child(f, p, IQ)
}
pub fn tilt(pitch: f64, roll: f64, yaw: f64) -> Q {
    qm(qm(qa(1, yaw), qa(0, pitch)), qa(2, roll))
}
pub fn model(f: Frame, s: V) -> [f32; 16] {
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
    matrix(compose(&[a, scale(s[0], s[1], s[2])]))
}
#[derive(Default)]
pub struct Resources {
    meshes: BTreeMap<String, Arc<Geometry>>,
}
impl Resources {
    pub fn retire_unused(&mut self) {
        self.meshes.retain(|_, mesh| Arc::strong_count(mesh) > 1);
    }
    pub fn mesh(&mut self, key: &str, build: impl FnOnce() -> Geometry) -> Arc<Geometry> {
        Arc::clone(
            self.meshes
                .entry(key.into())
                .or_insert_with(|| Arc::new(build())),
        )
    }
    pub fn draw(&mut self, id: String, shape: &str, c: u32, f: Frame, s: V) -> Draw {
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
        Draw {
            id,
            resource_key: key,
            geometry: mesh,
            model: model(f, s),
            material: Material {
                color: rgb(c),
                emissive: [0.; 3],
                opacity: 1.,
                unlit: false,
                double_sided: false,
                ground: None,
            },
        }
    }
    pub fn primitive(&mut self, id: String, shape: Primitive, c: u32, f: Frame) -> Draw {
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
        Draw {
            id,
            resource_key: key,
            geometry: mesh,
            model: model(f, [1.; 3]),
            material: Material {
                color: rgb(c),
                emissive: [0.; 3],
                opacity: 1.,
                unlit: false,
                double_sided: false,
                ground: None,
            },
        }
    }
}
#[derive(Clone, Copy, Debug)]
pub enum Primitive {
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
pub fn root(p: V, yaw: f64, pivot: f64, lay: Option<(Q, f64)>) -> Frame {
    let base = Frame { p, q: qa(1, yaw) };
    if let Some((q, lift)) = lay {
        let [x, y, z] = qr(q, [0., pivot, 0.]);
        child(base, [-x, lift - y, -z], q)
    } else {
        base
    }
}
