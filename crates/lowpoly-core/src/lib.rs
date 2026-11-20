//! The lowpoly document model: meshes, modelling operations, history and file formats.
//!
//! This crate is pure: it has no dependency on egui, wgpu or any platform API, so everything in
//! it is unit-tested on the host with `cargo test`.

pub mod cow_vec;
pub mod math;
pub mod mesh;
pub mod mesh_data;
pub mod planarity;
pub mod triangulate;
pub mod uv;
