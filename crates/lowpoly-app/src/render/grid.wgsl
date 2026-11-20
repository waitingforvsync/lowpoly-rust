// Coloured line segments for the ground grid and axes. Dashed lines discard every other
// half period along their length.
//
// Colours are sRGB-encoded and written as-is: the viewport texture is Rgba8Unorm, which egui
// samples as gamma-space colour.

struct Uniforms {
    view_proj: mat4x4<f32>,
}

@group(0) @binding(0) var<uniform> uniforms: Uniforms;

// World length of one dash plus one gap.
const DASH_PERIOD: f32 = 0.5;

struct VertexIn {
    @location(0) position: vec3<f32>,
    @location(1) color: vec4<f32>,
    // Distance along a dashed line, or negative for a solid line.
    @location(2) dash: f32,
}

struct VertexOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) color: vec4<f32>,
    @location(1) dash: f32,
}

@vertex
fn vs_main(in: VertexIn) -> VertexOut {
    var out: VertexOut;
    out.clip = uniforms.view_proj * vec4<f32>(in.position, 1.0);
    out.color = in.color;
    out.dash = in.dash;
    return out;
}

@fragment
fn fs_main(in: VertexOut) -> @location(0) vec4<f32> {
    if in.dash >= 0.0 && fract(in.dash / DASH_PERIOD) >= 0.5 {
        discard;
    }
    return in.color;
}
