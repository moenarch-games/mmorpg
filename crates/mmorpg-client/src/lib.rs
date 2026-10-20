#![forbid(unsafe_code)]

pub mod camera;
pub mod controls;
pub mod environment;
pub mod gamepad;
pub mod graphics;
pub mod hud;
pub mod interaction;
pub mod network;
pub mod online_identity;
pub mod picking;
pub mod presentation;
pub mod roster;
pub mod session;
pub mod ui;
pub mod unit_models;
pub mod world;
pub mod world_controls;

pub type ClientError = Box<dyn std::error::Error + Send + Sync>;
