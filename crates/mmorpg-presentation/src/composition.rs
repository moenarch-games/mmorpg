//! Composition kits (#591): an asset-tooling bundle whose descriptor lists, per piece it
//! places, the zone bodies (collider boxes, ramp wedges, lift pillars) the piece dresses. The
//! Battle Royale fortress kit (asset-tooling #445, `fortress-kit-descriptor-v1`) is the first;
//! later passes of it keep the same `composition` and `lodSelection` format.
//!
//! [`Descriptor::dress`] keeps an item only when every one of its bodies equals a body of the
//! zone content exactly, so a changed body keeps its graybox instead of misplaced art. Static
//! pieces are placed once; a lift piece follows its platform, its base at the platform's
//! centre minus its half height. Presentation only: the content stays the one definition of
//! the geometry.
use std::collections::BTreeSet;

use mmorpg_core::{Platform, PlatformId, Ramp, RampRise, UNITS_PER_METRE, ZoneContent};
use serde_json::Value;

use crate::{
    batching::matrix,
    geometry::{compose, ry, translate},
    lod::{self, EntryLods},
};

/// One zone body a composition item dresses, as the descriptor states it (units).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Body {
    /// A static collider box by id, centre and half extents.
    Collider {
        id: u32,
        position: [i32; 3],
        half_extents: [i32; 3],
    },
    /// A ramp wedge by body, bounding-box centre, half extents and rise.
    Ramp(Ramp),
    /// A lift pillar by its platform body, its centre at the low stop and half extents.
    Lift {
        body: u32,
        low: [i32; 3],
        half_extents: [i32; 3],
    },
}

impl Body {
    /// Whether `content` holds exactly this body.
    fn in_content(&self, content: &ZoneContent) -> bool {
        match self {
            Self::Collider {
                id,
                position,
                half_extents,
            } => content.definition().colliders().iter().any(|collider| {
                collider.id == *id
                    && collider.position == *position
                    && collider.half_extents == *half_extents
            }),
            Self::Ramp(ramp) => content.ramps().contains(ramp),
            Self::Lift {
                body,
                low,
                half_extents,
            } => platform(content, *body)
                .is_some_and(|lift| lift.low == *low && lift.half_extents == *half_extents),
        }
    }

    /// The zone body id.
    #[must_use]
    pub const fn id(&self) -> u32 {
        match self {
            Self::Collider { id, .. } | Self::Lift { body: id, .. } => *id,
            Self::Ramp(ramp) => ramp.body,
        }
    }
}

fn platform(content: &ZoneContent, body: u32) -> Option<&Platform> {
    content.platforms().iter().find(|lift| lift.body == body)
}

/// One piece the descriptor places.
#[derive(Clone, Debug, PartialEq)]
pub struct Item {
    /// The bundle entry key; its `default` variant is the finest level.
    pub key: String,
    /// Base centre, units; a lift piece's at the platform's low stop.
    pub origin: [i32; 3],
    /// Turn about +Y, degrees; 90 turns +Z to +X.
    pub yaw_degrees: f64,
    pub bodies: Vec<Body>,
}

/// The parts of a composition kit's descriptor the clients use.
#[derive(Clone, Debug, PartialEq)]
pub struct Descriptor {
    pub items: Vec<Item>,
    /// Every piece's levels of detail (`lodSelection` and per asset `lods`).
    pub lods: Vec<EntryLods>,
}

fn ints(value: &Value, what: &str) -> Result<[i32; 3], String> {
    let array = value
        .as_array()
        .filter(|array| array.len() == 3)
        .ok_or_else(|| format!("{what} is not three numbers"))?;
    let mut out = [0; 3];
    for (slot, value) in out.iter_mut().zip(array) {
        *slot = value
            .as_i64()
            .and_then(|value| i32::try_from(value).ok())
            .ok_or_else(|| format!("{what} is not whole units"))?;
    }
    Ok(out)
}

fn body(value: &Value) -> Result<Body, String> {
    let id = value["body"]
        .as_u64()
        .and_then(|id| u32::try_from(id).ok())
        .ok_or("a composition body without id")?;
    let position = ints(&value["position"], "a body position")?;
    let half_extents = ints(&value["halfExtents"], "a body's half extents")?;
    Ok(match value["kind"].as_str() {
        Some("collider") => Body::Collider {
            id,
            position,
            half_extents,
        },
        Some("ramp") => Body::Ramp(Ramp {
            body: id,
            position,
            half_extents,
            rises: match value["rises"].as_str() {
                Some("PosX") => RampRise::PosX,
                Some("NegX") => RampRise::NegX,
                Some("PosZ") => RampRise::PosZ,
                Some("NegZ") => RampRise::NegZ,
                other => return Err(format!("ramp {id} rises {other:?}")),
            },
        }),
        Some("lift") => Body::Lift {
            body: id,
            low: position,
            half_extents,
        },
        other => return Err(format!("body {id} is of unknown kind {other:?}")),
    })
}

impl Descriptor {
    /// Reads a composition kit's descriptor JSON.
    ///
    /// # Errors
    ///
    /// For malformed JSON, items, bodies or levels of detail.
    pub fn parse(json: &str) -> Result<Self, String> {
        let value: Value =
            serde_json::from_str(json).map_err(|error| format!("kit descriptor: {error}"))?;
        let items = value["composition"]["items"]
            .as_array()
            .ok_or("the kit descriptor has no composition items")?
            .iter()
            .map(|item| {
                let key = item["key"]
                    .as_str()
                    .ok_or("a composition item without key")?;
                Ok(Item {
                    key: key.to_owned(),
                    origin: ints(&item["origin"], key)?,
                    yaw_degrees: item["yawDegrees"]
                        .as_f64()
                        .ok_or_else(|| format!("{key} has no yawDegrees"))?,
                    bodies: item["bodies"]
                        .as_array()
                        .filter(|bodies| !bodies.is_empty())
                        .ok_or_else(|| format!("{key} dresses no bodies"))?
                        .iter()
                        .map(body)
                        .collect::<Result<_, String>>()?,
                })
            })
            .collect::<Result<_, String>>()?;
        Ok(Self {
            items,
            lods: lod::descriptor_lods(&value)?,
        })
    }

    /// The items whose every body is exactly one of `content`'s, placed.
    #[must_use]
    pub fn dress(&self, content: &ZoneContent) -> Dressing {
        let mut dressing = Dressing::default();
        for item in &self.items {
            if !item.bodies.iter().all(|body| body.in_content(content)) {
                continue;
            }
            let yaw = item.yaw_degrees.to_radians();
            let lift = match item.bodies.as_slice() {
                [Body::Lift { body, .. }] => platform(content, *body),
                _ => None,
            };
            match lift {
                Some(lift) => dressing.lifts.push(Lift {
                    entry: item.key.clone(),
                    platform: lift.id,
                    yaw,
                }),
                None => {
                    let [x, y, z] = item.origin.map(metres);
                    dressing.statics.push(Static {
                        entry: item.key.clone(),
                        model: matrix(compose(&[translate(x, y, z), ry(yaw)])),
                    });
                }
            }
            dressing.bodies.extend(item.bodies.iter().map(Body::id));
        }
        let used = dressing.entries();
        let lods = self
            .lods
            .iter()
            .filter(|entry| used.contains(&entry.entry.as_str()))
            .cloned()
            .collect();
        dressing.lods = lods;
        dressing
    }
}

fn metres(units: i32) -> f64 {
    f64::from(units) / f64::from(UNITS_PER_METRE)
}

/// A static piece, placed once.
#[derive(Clone, Debug, PartialEq)]
pub struct Static {
    pub entry: String,
    /// Column-major entry-to-world matrix, metres.
    pub model: [f32; 16],
}

/// A lift piece: its base follows the platform's centre minus its half height, turned by `yaw`.
#[derive(Clone, Debug, PartialEq)]
pub struct Lift {
    pub entry: String,
    pub platform: PlatformId,
    /// Radians about +Y.
    pub yaw: f64,
}

/// What a composition kit draws over one zone's content.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Dressing {
    pub statics: Vec<Static>,
    pub lifts: Vec<Lift>,
    /// Every body a drawn piece dresses; the rest keep their graybox.
    pub bodies: BTreeSet<u32>,
    /// The levels of detail of the entries drawn.
    pub lods: Vec<EntryLods>,
}

impl Dressing {
    /// The bundle entries drawn, sorted and once each.
    #[must_use]
    pub fn entries(&self) -> Vec<&str> {
        self.statics
            .iter()
            .map(|piece| piece.entry.as_str())
            .chain(self.lifts.iter().map(|piece| piece.entry.as_str()))
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect()
    }

    /// The platforms a lift piece draws (their procedural pillars are its fallback).
    #[must_use]
    pub fn platforms(&self) -> BTreeSet<PlatformId> {
        self.lifts.iter().map(|lift| lift.platform).collect()
    }

    /// The graybox props ([`crate::Kit::Graybox`]) over dressed bodies, sorted, for
    /// [`crate::Kits::dressed`]; `prop_bodies` is each graybox prop's body by prop index.
    #[must_use]
    pub fn props(&self, prop_bodies: &[Option<u32>]) -> Vec<usize> {
        prop_bodies
            .iter()
            .enumerate()
            .filter(|(_, body)| body.is_some_and(|body| self.bodies.contains(&body)))
            .map(|(prop, _)| prop)
            .collect()
    }
}
