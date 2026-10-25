//! Game-specific procedural shapes in metres: positions, normals and optional linear vertex colours.
use std::f64::consts::{PI, TAU};
pub type V = [f64; 3];
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Geometry {
    pub p: Vec<V>,
    pub n: Vec<V>,
    pub colors: Vec<V>,
    pub indices: Vec<u32>,
}
#[derive(Clone, Copy, Debug)]
pub struct Affine {
    pub m: [f64; 9],
    pub t: V,
}
pub const ID: Affine = Affine {
    m: [1., 0., 0., 0., 1., 0., 0., 0., 1.],
    t: [0.; 3],
};
pub fn translate(x: f64, y: f64, z: f64) -> Affine {
    Affine { t: [x, y, z], ..ID }
}
pub fn scale(x: f64, y: f64, z: f64) -> Affine {
    Affine {
        m: [x, 0., 0., 0., y, 0., 0., 0., z],
        ..ID
    }
}
pub fn ry(a: f64) -> Affine {
    let (c, s) = (a.cos(), a.sin());
    Affine {
        m: [c, 0., s, 0., 1., 0., -s, 0., c],
        ..ID
    }
}
pub fn rx(a: f64) -> Affine {
    let (c, s) = (a.cos(), a.sin());
    Affine {
        m: [1., 0., 0., 0., c, -s, 0., s, c],
        ..ID
    }
}
pub fn rz(a: f64) -> Affine {
    let (c, s) = (a.cos(), a.sin());
    Affine {
        m: [c, -s, 0., s, c, 0., 0., 0., 1.],
        ..ID
    }
}
pub fn point(a: Affine, p: V) -> V {
    std::array::from_fn(|i| {
        a.m[i * 3] * p[0] + a.m[i * 3 + 1] * p[1] + a.m[i * 3 + 2] * p[2] + a.t[i]
    })
}
pub fn compose(ts: &[Affine]) -> Affine {
    ts.iter().fold(ID, |a, b| {
        let m = std::array::from_fn(|i| {
            let (r, c) = (i / 3, i % 3);
            a.m[r * 3] * b.m[c] + a.m[r * 3 + 1] * b.m[c + 3] + a.m[r * 3 + 2] * b.m[c + 6]
        });
        Affine {
            m,
            t: point(a, b.t),
        }
    })
}
pub fn at(x: f64, y: f64, z: f64, sx: f64, sy: f64, sz: f64) -> Affine {
    compose(&[translate(x, y, z), scale(sx, sy, sz)])
}
pub fn norm(p: V) -> V {
    let l = (p[0] * p[0] + p[1] * p[1] + p[2] * p[2]).sqrt();
    if l > 0. {
        p.map(|v| v / l)
    } else {
        [0., 1., 0.]
    }
}
impl Geometry {
    pub fn add(&mut self, s: &Self, a: Affine) {
        let [aa, b, c, d, e, f, g, h, i] = a.m;
        let co = [
            e * i - f * h,
            f * g - d * i,
            d * h - e * g,
            c * h - b * i,
            aa * i - c * g,
            b * g - aa * h,
            b * f - c * e,
            c * d - aa * f,
            aa * e - b * d,
        ];
        let det = aa * co[0] + b * co[1] + c * co[2];
        let inv = if det == 0. { 0. } else { 1. / det };
        let base = self.p.len() as u32;
        self.p.extend(s.p.iter().map(|p| point(a, *p)));
        self.n.extend(s.n.iter().map(|n| {
            norm(std::array::from_fn(|r| {
                (co[r * 3] * n[0] + co[r * 3 + 1] * n[1] + co[r * 3 + 2] * n[2]) * inv
            }))
        }));
        for tri in s.indices.as_chunks::<3>().0 {
            self.indices.extend(if det < 0. {
                [base + tri[0], base + tri[2], base + tri[1]]
            } else {
                [base + tri[0], base + tri[1], base + tri[2]]
            });
        }
    }
    pub fn face(&mut self, c: &[V]) {
        let base = self.p.len() as u32;
        let u = std::array::from_fn::<_, 3, _>(|i| c[1][i] - c[0][i]);
        let v = std::array::from_fn::<_, 3, _>(|i| c[2][i] - c[0][i]);
        let n = norm([
            u[1] * v[2] - u[2] * v[1],
            u[2] * v[0] - u[0] * v[2],
            u[0] * v[1] - u[1] * v[0],
        ]);
        self.p.extend_from_slice(c);
        self.n.extend(std::iter::repeat_n(n, c.len()));
        for i in 1..c.len() - 1 {
            self.indices
                .extend([base, base + i as u32, base + i as u32 + 1]);
        }
    }
}
pub fn box_shape() -> Geometry {
    let mut g = Geometry::default();
    for c in [
        [[1., -1., -1.], [1., 1., -1.], [1., 1., 1.], [1., -1., 1.]],
        [
            [-1., -1., 1.],
            [-1., 1., 1.],
            [-1., 1., -1.],
            [-1., -1., -1.],
        ],
        [[-1., 1., -1.], [-1., 1., 1.], [1., 1., 1.], [1., 1., -1.]],
        [
            [-1., -1., 1.],
            [-1., -1., -1.],
            [1., -1., -1.],
            [1., -1., 1.],
        ],
        [[-1., -1., 1.], [1., -1., 1.], [1., 1., 1.], [-1., 1., 1.]],
        [
            [1., -1., -1.],
            [-1., -1., -1.],
            [-1., 1., -1.],
            [1., 1., -1.],
        ],
    ] {
        g.face(&c);
    }
    g
}
pub fn gable() -> Geometry {
    let mut g = Geometry::default();
    g.face(&[[-1., 0., 1.], [1., 0., 1.], [1., 1., 0.], [-1., 1., 0.]]);
    g.face(&[[1., 0., -1.], [-1., 0., -1.], [-1., 1., 0.], [1., 1., 0.]]);
    g.face(&[[1., 0., 1.], [1., 0., -1.], [1., 1., 0.]]);
    g.face(&[[-1., 0., -1.], [-1., 0., 1.], [-1., 1., 0.]]);
    g
}
/// A ramp's wedge (#59) in the box x, z ∈ [-1, 1], y ∈ [0, 1]: a flat bottom, a vertical
/// back face at z = 1 and a top rising from z = -1 at the ground to z = 1 at full height.
pub fn wedge() -> Geometry {
    let mut g = Geometry::default();
    g.face(&[[-1., 0., -1.], [-1., 1., 1.], [1., 1., 1.], [1., 0., -1.]]);
    g.face(&[[-1., 0., 1.], [1., 0., 1.], [1., 1., 1.], [-1., 1., 1.]]);
    g.face(&[[-1., 0., -1.], [1., 0., -1.], [1., 0., 1.], [-1., 0., 1.]]);
    g.face(&[[1., 0., -1.], [1., 1., 1.], [1., 0., 1.]]);
    g.face(&[[-1., 0., -1.], [-1., 0., 1.], [-1., 1., 1.]]);
    g
}
pub fn pyramid() -> Geometry {
    let mut g = Geometry::default();
    let c = [[-1., 0., -1.], [-1., 0., 1.], [1., 0., 1.], [1., 0., -1.]];
    for i in 0..4 {
        g.face(&[c[i], c[(i + 1) % 4], [0., 1., 0.]]);
    }
    g
}
pub fn blade() -> Geometry {
    let p = vec![
        [0.35, 0., 0.],
        [-0.2, 0., 0.3],
        [-0.2, 0., -0.3],
        [0., 1., 0.],
    ];
    let n = p
        .iter()
        .take(3)
        .map(|v| norm([v[0], 1.4, v[2]]))
        .chain([[0., 1., 0.]])
        .collect();
    Geometry {
        p,
        n,
        indices: vec![1, 0, 3, 2, 1, 3, 0, 2, 3],
        ..Default::default()
    }
}
pub fn frustum(sides: usize, top: f64, smooth: bool, caps: bool, offset: f64) -> Geometry {
    let mut g = Geometry::default();
    let ring = |r: f64, y: f64| -> Vec<V> {
        (0..sides)
            .map(|i| {
                let a = (i as f64 + offset) / sides as f64 * TAU;
                [a.sin() * r, y, a.cos() * r]
            })
            .collect()
    };
    let b = ring(1., 0.);
    let t = ring(top, 1.);
    if smooth {
        for i in 0..sides {
            let a = (i as f64 + offset) / sides as f64 * TAU;
            let n = norm([a.sin(), 1. - top, a.cos()]);
            g.p.extend([b[i], t[i]]);
            g.n.extend([n, n]);
        }
        for i in 0..sides {
            let a = i as u32 * 2;
            let b = ((i + 1) % sides) as u32 * 2;
            g.indices.extend([a, b, a + 1, a + 1, b, b + 1]);
        }
    } else {
        for i in 0..sides {
            let j = (i + 1) % sides;
            if top == 0. {
                g.face(&[b[i], b[j], t[i]]);
            } else {
                g.face(&[b[i], b[j], t[j], t[i]]);
            }
        }
    }
    if caps {
        g.face(&b.into_iter().rev().collect::<Vec<_>>());
        if top > 0. {
            g.face(&t);
        }
    }
    g
}
pub fn disc(n: usize) -> Geometry {
    let mut g = Geometry {
        p: vec![[0.; 3]],
        n: vec![[0., 1., 0.]],
        ..Default::default()
    };
    for i in 0..n {
        let a = i as f64 / n as f64 * TAU;
        g.p.push([a.sin(), 0., a.cos()]);
        g.n.push([0., 1., 0.]);
        g.indices
            .extend([0, 1 + i as u32, 1 + ((i + 1) % n) as u32]);
    }
    g
}
pub fn hash(values: &[f64]) -> f64 {
    let mut s = 0x9e3779b9u32;
    for v in values {
        s = (s ^ ((v + 0.5).floor() as i64 as u32)).wrapping_mul(0x85ebca6b);
        s ^= s >> 13;
        s = s.wrapping_mul(0xc2b2ae35);
        s ^= s >> 16;
    }
    f64::from(s) / 4294967296.
}
pub fn blob(detail: usize, smooth: bool, jitter: f64, seed: f64) -> Geometry {
    let t = (1. + 5f64.sqrt()) / 2.;
    let mut v = vec![
        [-1., t, 0.],
        [1., t, 0.],
        [-1., -t, 0.],
        [1., -t, 0.],
        [0., -1., t],
        [0., 1., t],
        [0., -1., -t],
        [0., 1., -t],
        [t, 0., -1.],
        [t, 0., 1.],
        [-t, 0., -1.],
        [-t, 0., 1.],
    ]
    .into_iter()
    .map(norm)
    .collect::<Vec<_>>();
    let mut faces = vec![
        [0, 11, 5],
        [0, 5, 1],
        [0, 1, 7],
        [0, 7, 10],
        [0, 10, 11],
        [1, 5, 9],
        [5, 11, 4],
        [11, 10, 2],
        [10, 7, 6],
        [7, 1, 8],
        [3, 9, 4],
        [3, 4, 2],
        [3, 2, 6],
        [3, 6, 8],
        [3, 8, 9],
        [4, 9, 5],
        [2, 4, 11],
        [6, 2, 10],
        [8, 6, 7],
        [9, 8, 1],
    ];
    if detail == 1 {
        let mut mids = std::collections::BTreeMap::new();
        let mut mid = |a: usize, b: usize| {
            let key = (a.min(b), a.max(b));
            *mids.entry(key).or_insert_with(|| {
                let p = norm(std::array::from_fn(|i| (v[a][i] + v[b][i]) / 2.));
                v.push(p);
                v.len() - 1
            })
        };
        faces = faces
            .into_iter()
            .flat_map(|[a, b, c]| {
                let (ab, bc, ca) = (mid(a, b), mid(b, c), mid(c, a));
                [[a, ab, ca], [b, bc, ab], [c, ca, bc], [ab, bc, ca]]
            })
            .collect();
    }
    let p: Vec<_> = v
        .iter()
        .enumerate()
        .map(|(i, p)| p.map(|x| x * (1. + (hash(&[seed, i as f64]) * 2. - 1.) * jitter)))
        .collect();
    if smooth {
        Geometry {
            p,
            n: v,
            indices: faces.iter().flatten().map(|i| *i as u32).collect(),
            ..Default::default()
        }
    } else {
        let mut g = Geometry::default();
        for [a, b, c] in faces {
            g.face(&[p[a], p[b], p[c]]);
        }
        g
    }
}
pub fn align_y(d: V) -> Affine {
    let l = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
    if l == 0. {
        return ID;
    }
    let [x, y, z] = d.map(|v| v / l);
    let s = x.hypot(z);
    if s < 1e-9 {
        return if y > 0. { ID } else { rx(PI) };
    }
    let (ax, az) = (z / s, -x / s);
    let t = 1. - y;
    Affine {
        m: [
            t * ax * ax + y,
            -s * az,
            t * ax * az,
            s * az,
            y,
            -s * ax,
            t * ax * az,
            s * ax,
            t * az * az + y,
        ],
        ..ID
    }
}
