//! Shared viewport colours, matching the TypeScript app. Axis colours follow SketchUp:
//! red = X, green = Y, blue = Z (up).

pub(crate) const BACKGROUND: u32 = 0x0026_282c;
pub(crate) const GRID_MINOR: u32 = 0x0034_373c;
pub(crate) const GRID_MAJOR: u32 = 0x0046_4a51;
pub(crate) const AXIS_X: u32 = 0x00d9_434b;
pub(crate) const AXIS_Y: u32 = 0x004c_ae50;
pub(crate) const AXIS_Z: u32 = 0x003f_7ee8;

/// Splits a `0xRRGGBB` colour into sRGB-encoded channels in 0..=1, with alpha 1.
pub(crate) fn rgba(hex: u32) -> [f32; 4] {
    let [_, r, g, b] = hex.to_be_bytes();
    [
        f32::from(r) / 255.0,
        f32::from(g) / 255.0,
        f32::from(b) / 255.0,
        1.0,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rgba_splits_channels() {
        assert_eq!(rgba(0x00ff_8000), [1.0, 128.0 / 255.0, 0.0, 1.0]);
    }
}
