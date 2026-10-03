//! Shared model for Needle and Thread. No I/O here: everything must also build for wasm32,
//! so the phone app and reader page can use it.

pub mod scene;
pub mod spell;
