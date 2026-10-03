//! Shared model for Needle and Thread. No I/O here: everything must also build for wasm32,
//! so the phone app and reader page can use it.

pub mod diff;
pub mod header;
pub mod id;
pub mod outline;
pub mod project;
pub mod scene;
pub mod settings;
pub mod spell;
pub mod words;
