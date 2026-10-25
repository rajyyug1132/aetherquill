//! Host-testable slice of the device crate: geometry, effects, SpellIr view
//! and resume detection — no qtfb/device deps.
//! The aetherquill bin (qtfb + evdev) is linux/armv7; this lib compiles
//! anywhere so `cargo test --lib` can emulate pen input off-device.
pub mod catalogue;
pub mod lag;
pub mod pen;
pub mod render;
pub mod resume;
pub mod session;
pub mod shapes;
pub mod spell_view;
