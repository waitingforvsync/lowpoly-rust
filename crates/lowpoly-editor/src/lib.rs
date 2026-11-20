//! The lowpoly editing session: camera, selection, tools and actions.
//!
//! Like `lowpoly-core`, this crate has no GPU or platform dependencies, so editor behaviour is
//! unit-tested on the host. The app crate feeds it input and draws what it describes.

mod camera;

pub use camera::{OrbitCamera, Projection};
