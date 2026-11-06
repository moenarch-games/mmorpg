//! WASM-only acceptance exports; the production adapter has no backend selector.

#[cfg(target_arch = "wasm32")]
#[path = "../tests/physics_ab.rs"]
mod acceptance;

#[cfg(target_arch = "wasm32")]
#[path = "../tests/authored_backend.rs"]
mod authored_backend;

#[cfg(target_arch = "wasm32")]
#[path = "../tests/authored_collision.rs"]
mod authored_collision;
