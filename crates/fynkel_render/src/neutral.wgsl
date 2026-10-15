// Khronos PBR Neutral tone mapping, as the forward pass does it
// (forward.wgsl's tonemap_neutral), and its inverse: the scene image holds
// tone-mapped colours, and effects that work on light (bloom) undo it.
// Put in front of the shaders that use it.

const START_COMPRESSION: f32 = 0.76;
const DESATURATION: f32 = 0.15;

fn tonemap(color_in: vec3<f32>) -> vec3<f32> {
    var color = color_in;
    let x = min(color.r, min(color.g, color.b));
    var offset = 0.04;
    if x < 0.08 {
        offset = x - 6.25 * x * x;
    }
    color -= offset;
    let peak = max(color.r, max(color.g, color.b));
    if peak < START_COMPRESSION {
        return color;
    }
    let d = 1.0 - START_COMPRESSION;
    let new_peak = 1.0 - d * d / (peak + d - START_COMPRESSION);
    color *= new_peak / peak;
    let g = 1.0 - 1.0 / (DESATURATION * (peak - new_peak) + 1.0);
    return mix(color, vec3<f32>(new_peak), g);
}

// The light `tonemap` turned into `mapped`. Near white the curve is all
// but flat, so half floats hold too little to tell very bright light
// apart: it comes back as at most about 115.
fn untonemap(mapped: vec3<f32>) -> vec3<f32> {
    var color = max(mapped, vec3<f32>(0.0));
    let top = max(color.r, max(color.g, color.b));
    if top >= START_COMPRESSION {
        let d = 1.0 - START_COMPRESSION;
        let new_peak = min(top, 0.9995);
        let peak = d * d / (1.0 - new_peak) - d + START_COMPRESSION;
        let g = 1.0 - 1.0 / (DESATURATION * (peak - new_peak) + 1.0);
        color = max(color - new_peak * g, vec3<f32>(0.0)) / (1.0 - g) * (peak / new_peak);
    }
    // The offset: the darkest channel y was x - (x - 6.25x²) = 6.25x²
    // below 0.08, x - 0.04 above.
    let y = min(color.r, min(color.g, color.b));
    var offset = 0.04;
    if y < 0.04 {
        let x = sqrt(y) / 2.5;
        offset = x - y;
    }
    return color + offset;
}
