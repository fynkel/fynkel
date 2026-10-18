// Helpers for any shader (stage 10), a material's or a screen effect's:
// after library.wgsl, or effect.wgsl.

// A number from 0 to 1 that looks random, the same for the same `p`.
fn hash(p: vec2<f32>) -> f32 {
    let q = fract(p * vec2<f32>(0.1031, 0.1030));
    let r = q + dot(q, q.yx + 33.33);
    return fract((r.x + r.y) * r.x);
}

fn hash3(p: vec3<f32>) -> f32 {
    var q = fract(p * 0.1031);
    q += dot(q, q.zyx + 31.32);
    return fract((q.x + q.y) * q.z);
}

// Smooth noise from 0 to 1: about one bump each 1 of `p`.
fn noise(p: vec2<f32>) -> f32 {
    let i = floor(p);
    let f = fract(p);
    let u = f * f * (3.0 - 2.0 * f);
    let a = hash(i);
    let b = hash(i + vec2<f32>(1.0, 0.0));
    let c = hash(i + vec2<f32>(0.0, 1.0));
    let d = hash(i + vec2<f32>(1.0, 1.0));
    return mix(mix(a, b, u.x), mix(c, d, u.x), u.y);
}

// `noise` in 3D: for a solid that's the same all through.
fn noise3(p: vec3<f32>) -> f32 {
    let i = floor(p);
    let f = fract(p);
    let u = f * f * (3.0 - 2.0 * f);
    let x0 = mix(hash3(i), hash3(i + vec3<f32>(1.0, 0.0, 0.0)), u.x);
    let x1 = mix(hash3(i + vec3<f32>(0.0, 1.0, 0.0)), hash3(i + vec3<f32>(1.0, 1.0, 0.0)), u.x);
    let x2 = mix(hash3(i + vec3<f32>(0.0, 0.0, 1.0)), hash3(i + vec3<f32>(1.0, 0.0, 1.0)), u.x);
    let x3 = mix(hash3(i + vec3<f32>(0.0, 1.0, 1.0)), hash3(i + vec3<f32>(1.0, 1.0, 1.0)), u.x);
    return mix(mix(x0, x1, u.y), mix(x2, x3, u.y), u.z);
}

// Noise with detail: four of `noise`, each twice as fine and half as
// strong as the last. 0 to 1.
fn fbm(p: vec2<f32>) -> f32 {
    var sum = 0.0;
    var strength = 0.5;
    var q = p;
    for (var i = 0; i < 4; i++) {
        sum += noise(q) * strength;
        q = q * 2.03 + vec2<f32>(17.0, 31.0);
        strength *= 0.5;
    }
    return sum / 0.9375;
}

// `x` from the range `a` to `b` moved to `c` to `d`, not clamped.
fn remap(x: f32, a: f32, b: f32, c: f32, d: f32) -> f32 {
    return c + (x - a) * (d - c) / (b - a);
}
