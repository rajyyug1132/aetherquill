//! Host-testable slice of the device crate: pure stroke geometry only.
//! The aetherquill bin (qtfb + evdev) is linux/armv7; this lib compiles
//! anywhere so `cargo test --lib` can emulate pen input off-device.
pub mod render;
pub mod shapes;
