//! WASM-only acceptance exports; the production adapter has no backend selector.

#[cfg(target_arch = "wasm32")]
#[path = "../tests/physics_ab.rs"]
mod acceptance;
