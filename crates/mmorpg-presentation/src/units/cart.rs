//! A merchant's handcart (#992): a merchant whose projected posture is
//! [`mmorpg_core::PostureView::Hauling`] pushes a two-wheeled cart of wares in front of it, in the
//! timber, iron and canvas of the Vale's standing carts. The cart follows the merchant's rendered
//! feet and heading, and its wheels turn with the ground it covers, so both clients draw the same
//! cart from the projection alone.
use std::f64::consts::FRAC_PI_2;

use super::kit::{Frame, Primitive, Resources, child, qa};
use crate::Draw;

const WOOD: u32 = 0x8a6a45;
const DARK_WOOD: u32 = 0x5e4630;
const IRON: u32 = 0x4b4d52;
const CRATE: u32 = 0xa88452;
const CANVAS: u32 = 0xc9b48a;

/// Wheel radius in metres.
const WHEEL: f64 = 0.38;

/// The handcart of the merchant standing at `feet` (metres) facing `yaw` (radians, 0 facing +Z,
/// turning toward +X), its wheels turned for `travelled` metres.
pub fn handcart(
    resources: &mut Resources,
    id: &str,
    feet: [f64; 3],
    yaw: f64,
    travelled: f64,
) -> Vec<Draw> {
    let root = Frame {
        p: feet,
        q: qa(1, yaw),
    };
    // The cart's body sits ahead of the merchant, its shafts reaching back to its hands.
    let body = child(root, [0., 0., 1.45], qa(1, 0.));
    let part = |name: &str| format!("{id}-cart-{name}");
    let mut draws = vec![
        resources.primitive(
            part("bed"),
            Primitive::Box([1.1, 0.1, 1.3]),
            WOOD,
            child(body, [0., 0.62, 0.], qa(1, 0.)),
        ),
        resources.primitive(
            part("front"),
            Primitive::Box([1.1, 0.34, 0.06]),
            DARK_WOOD,
            child(body, [0., 0.84, 0.62], qa(1, 0.)),
        ),
        resources.primitive(
            part("crate"),
            Primitive::Box([0.5, 0.4, 0.5]),
            CRATE,
            child(body, [-0.24, 0.87, 0.25], qa(1, 0.3)),
        ),
        resources.primitive(
            part("sack"),
            Primitive::Ball(0.24),
            CANVAS,
            child(body, [0.26, 0.86, -0.15], qa(1, 0.)),
        ),
        resources.primitive(
            part("bundle"),
            Primitive::Box([0.42, 0.22, 0.36]),
            CANVAS,
            child(body, [0.22, 0.78, 0.32], qa(1, -0.2)),
        ),
    ];
    for (side, x) in [("l", -0.62), ("r", 0.62)] {
        draws.push(resources.primitive(
            part(&format!("rail-{side}")),
            Primitive::Box([0.06, 0.3, 1.3]),
            DARK_WOOD,
            child(body, [x * 0.92, 0.82, 0.], qa(1, 0.)),
        ));
        // The wheel's axle runs across the cart; it rolls with the ground covered.
        let hub = child(body, [x, WHEEL, 0.05], qa(0, travelled / WHEEL));
        draws.push(resources.primitive(
            part(&format!("wheel-{side}")),
            Primitive::Post(WHEEL, 0.07),
            DARK_WOOD,
            child(hub, [0., 0., 0.], qa(2, FRAC_PI_2)),
        ));
        draws.push(resources.primitive(
            part(&format!("spoke-{side}")),
            Primitive::Box([0.08, WHEEL * 1.7, 0.05]),
            IRON,
            child(hub, [x.signum() * 0.05, 0., 0.], qa(1, 0.)),
        ));
        // A shaft from the bed back to the merchant's hands.
        draws.push(resources.primitive(
            part(&format!("shaft-{side}")),
            Primitive::Box([0.05, 0.05, 1.05]),
            WOOD,
            child(root, [x * 0.55, 0.8, 0.55], qa(0, -0.12)),
        ));
    }
    draws
}
