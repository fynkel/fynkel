// Four colours, as an old handheld's screen: each pixel takes the one
// nearest its brightness.
struct Values { darkest: vec3<f32>, dark: vec3<f32>, light: vec3<f32>, lightest: vec3<f32> }

fn effect(in: EffectInput) -> vec4<f32> {
    let seen = picture(in.uv).rgb;
    // Brightness as the eye sees it, roughly in sRGB's steps.
    let bright = sqrt(dot(seen, vec3<f32>(0.2126, 0.7152, 0.0722)));
    let step = clamp(i32(bright * 4.0), 0, 3);
    var colors = array<vec3<f32>, 4>(values.darkest, values.dark, values.light, values.lightest);
    return vec4<f32>(colors[step], 1.0);
}
