//! Ground grid on the XY plane with axis lines. Positive axes are solid and negative axes are
//! dashed, as in SketchUp.

use std::ops::Range;

use bytemuck::{Pod, Zeroable};
use eframe::wgpu;

use super::colors;

/// Half the grid's side length, in world units.
const EXTENT: i16 = 50;
/// Every this many lines is drawn in the major colour.
const MAJOR_EVERY: i16 = 10;

/// One end of a grid or axis line.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Pod, Zeroable)]
pub(crate) struct LineVertex {
    position: [f32; 3],
    color: [f32; 4],
    /// Distance from the start of a dashed line, or -1 for a solid line.
    dash: f32,
}

impl LineVertex {
    const SOLID: f32 = -1.0;

    pub(crate) const LAYOUT: wgpu::VertexBufferLayout<'static> = wgpu::VertexBufferLayout {
        array_stride: size_of::<Self>() as wgpu::BufferAddress,
        step_mode: wgpu::VertexStepMode::Vertex,
        attributes: &wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x4, 2 => Float32],
    };
}

/// Grid line vertices, as a line list, split by how they use depth.
#[derive(Debug)]
pub(crate) struct GridGeometry {
    pub(crate) vertices: Vec<LineVertex>,
    /// Lines on the ground plane. They're drawn first without writing depth, so geometry
    /// sitting on z = 0 covers them instead of z-fighting.
    pub(crate) ground: Range<u32>,
    /// The vertical axis, which depth-tests and writes depth normally.
    pub(crate) vertical: Range<u32>,
}

impl GridGeometry {
    pub(crate) fn new() -> Self {
        let extent = f32::from(EXTENT);
        let mut vertices = Vec::new();
        let mut line = |a: [f32; 3], b: [f32; 3], hex: u32| {
            let color = colors::rgba(hex);
            vertices.push(LineVertex {
                position: a,
                color,
                dash: LineVertex::SOLID,
            });
            vertices.push(LineVertex {
                position: b,
                color,
                dash: LineVertex::SOLID,
            });
        };

        for i in -EXTENT..=EXTENT {
            if i == 0 {
                continue; // the axis lines are drawn instead
            }
            let hex = if i % MAJOR_EVERY == 0 {
                colors::GRID_MAJOR
            } else {
                colors::GRID_MINOR
            };
            let i = f32::from(i);
            line([i, -extent, 0.0], [i, extent, 0.0], hex); // parallel to Y
            line([-extent, i, 0.0], [extent, i, 0.0], hex); // parallel to X
        }

        let axis = |vertices: &mut Vec<LineVertex>, dir: [f32; 3], hex: u32| {
            let color = colors::rgba(hex);
            let end = dir.map(|c| c * extent);
            let origin = LineVertex {
                position: [0.0; 3],
                color,
                dash: LineVertex::SOLID,
            };
            vertices.push(origin);
            vertices.push(LineVertex {
                position: end,
                ..origin
            });
            // The negative half is dashed, measured from the origin.
            vertices.push(LineVertex {
                dash: 0.0,
                ..origin
            });
            vertices.push(LineVertex {
                position: end.map(|c| -c),
                color,
                dash: extent,
            });
        };
        axis(&mut vertices, [1.0, 0.0, 0.0], colors::AXIS_X);
        axis(&mut vertices, [0.0, 1.0, 0.0], colors::AXIS_Y);
        let ground_end = vertex_count(&vertices);
        axis(&mut vertices, [0.0, 0.0, 1.0], colors::AXIS_Z);
        let end = vertex_count(&vertices);

        Self {
            vertices,
            ground: 0..ground_end,
            vertical: ground_end..end,
        }
    }
}

fn vertex_count(vertices: &[LineVertex]) -> u32 {
    u32::try_from(vertices.len()).expect("grid has fewer than 2^32 vertices")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ground_lines_lie_on_ground_and_vertical_axis_is_separate() {
        let grid = GridGeometry::new();
        let ground = &grid.vertices[grid.ground.start as usize..grid.ground.end as usize];
        assert!(
            ground.iter().all(|v| v.position[2] == 0.0),
            "ground lines on z = 0"
        );
        let vertical = &grid.vertices[grid.vertical.start as usize..grid.vertical.end as usize];
        assert_eq!(vertical.len(), 4, "one solid and one dashed segment");
        assert!(
            vertical.iter().all(|v| v.position[..2] == [0.0, 0.0]),
            "vertical axis on z"
        );
    }

    #[test]
    fn grid_skips_the_axis_lines() {
        let grid = GridGeometry::new();
        // 100 lines in each direction (-50..=50 without 0), 2 vertices each, plus 2 ground axes
        // of 2 segments.
        let lines = usize::try_from(EXTENT).unwrap() * 2 * 2;
        assert_eq!(grid.ground.len(), lines * 2 + 8);
    }

    #[test]
    fn negative_axes_are_dashed() {
        let grid = GridGeometry::new();
        // Grid lines never pass through an axis, so a point with one non-zero coordinate is an
        // axis end.
        for v in &grid.vertices {
            let on_axis = v.position.iter().filter(|&&c| c != 0.0).count() == 1;
            let negative = v.position.iter().any(|&c| c < 0.0);
            if on_axis {
                assert_eq!(v.dash >= 0.0, negative, "axis end {:?}", v.position);
            }
        }
    }
}
