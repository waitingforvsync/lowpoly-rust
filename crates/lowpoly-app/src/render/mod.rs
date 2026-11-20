//! wgpu rendering of the 3D viewport.
//!
//! Everything here is written to the WebGL2 downlevel limits: no compute, no storage buffers, and
//! 1 px line primitives.

mod colors;
mod grid;
mod viewport;

pub(crate) use viewport::ViewportRenderer;
