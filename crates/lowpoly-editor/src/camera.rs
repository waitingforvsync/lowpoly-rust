//! Orbit camera for the Z-up viewport.

use std::f64::consts::{PI, TAU};

use glam::{DMat4, DVec2, DVec3, DVec4};

/// Vertical field of view of the perspective projection.
const FOV_Y: f64 = 45.0 * PI / 180.0;

/// Near clip distance. The far plane is at infinity (reverse-Z).
const NEAR: f64 = 0.01;

const MIN_DISTANCE: f64 = 0.1;
const MAX_DISTANCE: f64 = 1000.0;

/// The orthographic eye sits this far back from the target, so the scene isn't clipped.
const ORTHO_EYE_DISTANCE: f64 = 500.0;
const ORTHO_FAR: f64 = ORTHO_EYE_DISTANCE * 4.0;

/// Keeps the camera just short of looking straight down or up, where yaw is undefined.
const MAX_PITCH: f64 = PI / 2.0 - 1e-4;

/// How the camera projects the scene.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Projection {
    /// Perspective with a 45° vertical field of view.
    #[default]
    Perspective,
    /// Orthographic, showing the same area at the target as the perspective view would.
    Orthographic,
}

/// A camera orbiting a target point, with +Z up.
///
/// The camera is described by its target, its distance from the target, and the direction it
/// looks from (yaw about +Z, measured from +X, and pitch above the ground plane). All matrices
/// use wgpu conventions with **reverse-Z** depth: near maps to 1, far to 0, so render passes clear
/// depth to 0 and test with `Greater`.
#[derive(Clone, Debug, PartialEq)]
pub struct OrbitCamera {
    target: DVec3,
    distance: f64,
    yaw: f64,
    pitch: f64,
    projection: Projection,
}

impl Default for OrbitCamera {
    /// The starting view: looking from the front-right, slightly above the origin.
    fn default() -> Self {
        Self::new(DVec3::new(4.0, -5.0, 3.5), DVec3::new(0.0, 0.0, 0.5))
    }
}

impl OrbitCamera {
    /// Creates a perspective camera at `eye`, looking at `target`.
    pub fn new(eye: DVec3, target: DVec3) -> Self {
        let offset = eye - target;
        let distance = offset.length().clamp(MIN_DISTANCE, MAX_DISTANCE);
        let horizontal = offset.truncate().length();
        Self {
            target,
            distance,
            yaw: offset.y.atan2(offset.x),
            pitch: offset.z.atan2(horizontal).clamp(-MAX_PITCH, MAX_PITCH),
            projection: Projection::Perspective,
        }
    }

    /// The point the camera orbits around and looks at.
    pub fn target(&self) -> DVec3 {
        self.target
    }

    /// Distance from the target. In orthographic mode this sets the visible area, not the
    /// eye position.
    pub fn distance(&self) -> f64 {
        self.distance
    }

    /// The current projection.
    pub fn projection(&self) -> Projection {
        self.projection
    }

    /// Switches between perspective and orthographic, keeping the visible area at the target.
    pub fn set_projection(&mut self, projection: Projection) {
        self.projection = projection;
    }

    /// Unit vector from the target towards the camera.
    fn back(&self) -> DVec3 {
        let (sin_yaw, cos_yaw) = self.yaw.sin_cos();
        let (sin_pitch, cos_pitch) = self.pitch.sin_cos();
        DVec3::new(cos_pitch * cos_yaw, cos_pitch * sin_yaw, sin_pitch)
    }

    /// Screen-right and screen-up directions in world space.
    fn screen_axes(&self) -> (DVec3, DVec3) {
        let forward = -self.back();
        let right = forward.cross(DVec3::Z).normalize();
        let up = right.cross(forward);
        (right, up)
    }

    /// The eye position used for rendering.
    pub fn eye(&self) -> DVec3 {
        let distance = match self.projection {
            Projection::Perspective => self.distance,
            Projection::Orthographic => ORTHO_EYE_DISTANCE,
        };
        self.target + self.back() * distance
    }

    /// Half the visible width and height, in world units, at the target's depth.
    fn half_extent(&self, aspect: f64) -> DVec2 {
        let half_height = self.distance * (FOV_Y / 2.0).tan();
        DVec2::new(half_height * aspect, half_height)
    }

    /// World-to-view transform (right-handed view space: x right, y up, looking down -z).
    pub fn view(&self) -> DMat4 {
        let (right, up) = self.screen_axes();
        let back = self.back();
        let eye = self.eye();
        DMat4::from_cols(
            DVec4::new(right.x, up.x, back.x, 0.0),
            DVec4::new(right.y, up.y, back.y, 0.0),
            DVec4::new(right.z, up.z, back.z, 0.0),
            DVec4::new(-right.dot(eye), -up.dot(eye), -back.dot(eye), 1.0),
        )
    }

    /// View-to-clip transform for a viewport with the given width / height `aspect`, with
    /// depth in 0..=1 (wgpu) and reversed.
    pub fn projection_matrix(&self, aspect: f64) -> DMat4 {
        match self.projection {
            Projection::Perspective => {
                // Infinite far plane: depth = NEAR / view distance, so NEAR maps to 1 and
                // infinity to 0.
                let f = 1.0 / (FOV_Y / 2.0).tan();
                DMat4::from_cols(
                    DVec4::new(f / aspect, 0.0, 0.0, 0.0),
                    DVec4::new(0.0, f, 0.0, 0.0),
                    DVec4::new(0.0, 0.0, 0.0, -1.0),
                    DVec4::new(0.0, 0.0, NEAR, 0.0),
                )
            }
            Projection::Orthographic => {
                // depth = (z + FAR) / (FAR - NEAR): view z = -NEAR maps to 1, -FAR to 0.
                let half = self.half_extent(aspect);
                let depth_range = ORTHO_FAR - NEAR;
                DMat4::from_cols(
                    DVec4::new(1.0 / half.x, 0.0, 0.0, 0.0),
                    DVec4::new(0.0, 1.0 / half.y, 0.0, 0.0),
                    DVec4::new(0.0, 0.0, 1.0 / depth_range, 0.0),
                    DVec4::new(0.0, 0.0, ORTHO_FAR / depth_range, 1.0),
                )
            }
        }
    }

    /// World-to-clip transform for a viewport with the given width / height `aspect`.
    pub fn view_projection(&self, aspect: f64) -> DMat4 {
        self.projection_matrix(aspect) * self.view()
    }

    /// Orbits around the target for a pointer drag of `delta_px` pixels (screen y down).
    ///
    /// Dragging the full viewport height turns the camera through one full revolution.
    pub fn orbit(&mut self, delta_px: DVec2, viewport_height_px: f64) {
        let radians_per_px = TAU / viewport_height_px.max(1.0);
        self.yaw = (self.yaw - delta_px.x * radians_per_px).rem_euclid(TAU);
        self.pitch = (self.pitch + delta_px.y * radians_per_px).clamp(-MAX_PITCH, MAX_PITCH);
    }

    /// Pans for a pointer drag of `delta_px` pixels (screen y down), so the point under the
    /// pointer at the target's depth follows it.
    pub fn pan(&mut self, delta_px: DVec2, viewport_height_px: f64) {
        let world_per_px = 2.0 * self.half_extent(1.0).y / viewport_height_px.max(1.0);
        let (right, up) = self.screen_axes();
        self.target += (up * delta_px.y - right * delta_px.x) * world_per_px;
    }

    /// Zooms by `factor` (below 1 zooms in) towards the point under `cursor_ndc`, which stays
    /// fixed on screen. The distance is clamped to the allowed range.
    pub fn zoom_at(&mut self, factor: f64, cursor_ndc: DVec2, aspect: f64) {
        let new_distance = (self.distance * factor).clamp(MIN_DISTANCE, MAX_DISTANCE);
        let scale = new_distance / self.distance;
        let half = self.half_extent(aspect);
        let (right, up) = self.screen_axes();
        let anchor = self.target + right * (cursor_ndc.x * half.x) + up * (cursor_ndc.y * half.y);
        // Scaling about the anchor keeps it at the same screen position, in both projections.
        self.target = anchor + (self.target - anchor) * scale;
        self.distance = new_distance;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ASPECT: f64 = 16.0 / 9.0;

    fn ndc(camera: &OrbitCamera, point: DVec3) -> DVec3 {
        camera.view_projection(ASPECT).project_point3(point)
    }

    fn assert_close(got: DVec3, want: DVec3) {
        assert!(got.abs_diff_eq(want, 1e-9), "got {got}, want {want}");
    }

    fn both_projections() -> [OrbitCamera; 2] {
        let mut ortho = OrbitCamera::default();
        ortho.set_projection(Projection::Orthographic);
        [OrbitCamera::default(), ortho]
    }

    #[test]
    fn default_view_matches_original_app() {
        let camera = OrbitCamera::default();
        assert_close(camera.eye(), DVec3::new(4.0, -5.0, 3.5));
        assert_close(camera.target(), DVec3::new(0.0, 0.0, 0.5));
    }

    #[test]
    fn target_is_at_screen_centre() {
        for camera in both_projections() {
            let p = ndc(&camera, camera.target());
            assert!(
                p.truncate().abs_diff_eq(DVec2::ZERO, 1e-9),
                "{:?}: {p}",
                camera.projection()
            );
        }
    }

    #[test]
    fn world_up_is_screen_up() {
        let camera = OrbitCamera::default();
        let below = ndc(&camera, camera.target());
        let above = ndc(&camera, camera.target() + DVec3::Z * 0.1);
        assert!(above.y > below.y, "{above} should be above {below}");
    }

    #[test]
    fn depth_is_reversed() {
        for camera in both_projections() {
            let near = ndc(&camera, camera.target() + camera.back() * 0.5);
            let far = ndc(&camera, camera.target() - camera.back() * 0.5);
            assert!(
                near.z > far.z,
                "{:?}: near {near} far {far}",
                camera.projection()
            );
            assert!(
                (0.0..=1.0).contains(&near.z) && (0.0..=1.0).contains(&far.z),
                "depth in range"
            );
        }
    }

    #[test]
    fn zoom_keeps_point_under_cursor_fixed() {
        let cursor = DVec2::new(0.6, -0.3);
        for mut camera in both_projections() {
            let half = camera.half_extent(ASPECT);
            let (right, up) = camera.screen_axes();
            let anchor = camera.target() + right * (cursor.x * half.x) + up * (cursor.y * half.y);
            assert_close(
                ndc(&camera, anchor).truncate().extend(0.0),
                cursor.extend(0.0),
            );

            camera.zoom_at(0.5, cursor, ASPECT);
            let after = ndc(&camera, anchor);
            assert!(
                after.truncate().abs_diff_eq(cursor, 1e-9),
                "{:?}: {after}",
                camera.projection()
            );
        }
    }

    #[test]
    fn zoom_clamps_distance() {
        let mut camera = OrbitCamera::default();
        camera.zoom_at(1e-9, DVec2::ZERO, ASPECT);
        assert!(
            (camera.distance() - MIN_DISTANCE).abs() < 1e-12,
            "{}",
            camera.distance()
        );
        camera.zoom_at(1e12, DVec2::ZERO, ASPECT);
        assert!(
            (camera.distance() - MAX_DISTANCE).abs() < 1e-9,
            "{}",
            camera.distance()
        );
    }

    #[test]
    fn pan_moves_scene_with_pointer() {
        let height = 900.0;
        let width = height * ASPECT;
        for mut camera in both_projections() {
            let old_target = camera.target();
            // Dragging right by half the viewport moves the old centre to the right edge.
            camera.pan(DVec2::new(width / 2.0, 0.0), height);
            let p = ndc(&camera, old_target);
            assert!(
                p.truncate().abs_diff_eq(DVec2::new(1.0, 0.0), 1e-9),
                "{:?}: {p}",
                camera.projection()
            );
        }
    }

    #[test]
    fn orbit_clamps_pitch_short_of_vertical() {
        let mut camera = OrbitCamera::default();
        camera.orbit(DVec2::new(0.0, 1e6), 900.0);
        assert!(camera.pitch <= MAX_PITCH, "pitch {}", camera.pitch);
        let view = camera.view();
        assert!(
            view.is_finite(),
            "view matrix stays finite looking straight down"
        );
    }

    #[test]
    fn orbit_full_height_drag_is_one_revolution() {
        let mut camera = OrbitCamera::default();
        let before = camera.eye();
        camera.orbit(DVec2::new(900.0, 0.0), 900.0);
        assert_close(camera.eye(), before);
    }
}
