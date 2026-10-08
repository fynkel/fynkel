//! Shared by the renderer's tests: a GPU, reading pixels back, golden image
//! comparison, and a CPU copy of the shading maths.
#![allow(dead_code, unused_imports, unreachable_pub)]

use fynkel_render::glam::{Mat4, Vec3};
use fynkel_render::*;

pub use fynkel_golden::{Image, assert_golden};

pub const CUBE: MeshKey = MeshKey(1);
pub const SPHERE: MeshKey = MeshKey(2);
pub const PLANE: MeshKey = MeshKey(3);

/// The GPU. A missing GPU fails the test rather than skipping it, so CI
/// can't go green without rendering anything.
pub fn gpu() -> Gpu {
    Gpu::headless().expect("these tests need a GPU")
}

/// A renderer for `format` with the three built-in shapes uploaded.
pub fn renderer(gpu: &Gpu, format: wgpu::TextureFormat) -> Renderer {
    let mut r = Renderer::new(gpu, format);
    r.upload_mesh(CUBE, &MeshData::cube());
    r.upload_mesh(SPHERE, &MeshData::sphere(48, 24));
    r.upload_mesh(PLANE, &MeshData::plane());
    r
}

/// Renders `scene` into a fresh texture of the renderer's format and reads
/// it back as RGBA.
pub fn render(gpu: &Gpu, renderer: &mut Renderer, scene: &RenderScene, width: u32, height: u32) -> (Image, FrameStats) {
    let format = renderer.format();
    render_into(gpu, renderer, scene, width, height, format)
}

/// Renders into a texture of `texture_format` through a view in the
/// renderer's (sRGB) format, as the platform targets do with plain-format
/// surfaces.
pub fn render_into(
    gpu: &Gpu,
    renderer: &mut Renderer,
    scene: &RenderScene,
    width: u32,
    height: u32,
    texture_format: wgpu::TextureFormat,
) -> (Image, FrameStats) {
    let view_format = renderer.format();
    let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("test target"),
        size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: texture_format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[view_format],
    });
    let view = texture.create_view(&wgpu::TextureViewDescriptor { format: Some(view_format), ..Default::default() });
    let stats = renderer.render(scene, &view, width, height);
    (read_back(gpu, &texture), stats)
}

/// Copies a texture to the CPU as RGBA8 (BGRA formats are swizzled).
pub fn read_back(gpu: &Gpu, texture: &wgpu::Texture) -> Image {
    let (width, height) = (texture.width(), texture.height());
    let padded = (width * 4).div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT) * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
    let buffer = gpu.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("readback"),
        size: (padded * height) as u64,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = gpu.device.create_command_encoder(&Default::default());
    encoder.copy_texture_to_buffer(
        texture.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(padded), rows_per_image: None },
        },
        texture.size(),
    );
    gpu.queue.submit([encoder.finish()]);
    buffer.slice(..).map_async(wgpu::MapMode::Read, |r| r.expect("map readback"));
    gpu.device.poll(wgpu::PollType::wait_indefinitely()).expect("poll");
    let data = buffer.slice(..).get_mapped_range().expect("mapped");
    let bgra = matches!(texture.format(), wgpu::TextureFormat::Bgra8Unorm | wgpu::TextureFormat::Bgra8UnormSrgb);
    let mut rgba = Vec::with_capacity((width * height * 4) as usize);
    for row in data.chunks(padded as usize).take(height as usize) {
        for px in row[..(width * 4) as usize].chunks(4) {
            if bgra {
                rgba.extend([px[2], px[1], px[0], px[3]]);
            } else {
                rgba.extend_from_slice(px);
            }
        }
    }
    Image { width, height, rgba }
}

// --- Scenes ----------------------------------------------------------------

/// sRGB → linear, as `fynkel_world::Color::to_linear` does.
pub fn linear(srgb: f32) -> f32 {
    if srgb <= 0.040_45 { srgb / 12.92 } else { ((srgb + 0.055) / 1.055).powf(2.4) }
}

pub fn linear_rgb(argb: u32) -> [f32; 4] {
    let c = |s: u32| linear(((argb >> s) & 0xFF) as f32 / 255.0);
    [c(16), c(8), c(0), 1.0]
}

/// The stage 1 plan's opening scene: the orange cube, turned 30° so three
/// faces show, under a sun and a little ambient light.
pub fn stage1_scene() -> RenderScene {
    let mut camera = CameraView::looking_at(Vec3::new(0.0, 1.5, 4.0), Vec3::ZERO);
    camera.clear_color = [linear(0.1), linear(0.1), linear(0.12)];
    RenderScene {
        camera: Some(camera),
        sun: Some(Sun { direction: Vec3::new(-0.4, -1.0, -0.3), color: Vec3::ONE }),
        ambient: Vec3::splat(0.15),
        draws: vec![Draw {
            mesh: CUBE,
            model: Mat4::from_rotation_y(30f32.to_radians()),
            material: PbrMaterial {
                base_color: linear_rgb(0xFFE0613A),
                metallic: 0.0,
                roughness: 0.5,
                ..PbrMaterial::DEFAULT
            },
        }],
    }
}

// --- The shading maths on the CPU ---------------------------------------------
//
// A second, independent copy of forward.wgsl's maths, written from the same
// formulas. The units test checks the GPU against it.

const PI: f32 = std::f32::consts::PI;

pub struct Surface {
    pub albedo: Vec3,
    pub metallic: f32,
    pub roughness: f32,
}

/// Linear radiance leaving a surface with normal `n`, seen from `v`, lit by
/// a sun towards `l` and by ambient light.
pub fn shade(s: &Surface, n: Vec3, v: Vec3, l: Vec3, sun: Vec3, ambient: Vec3) -> Vec3 {
    let roughness = s.roughness.clamp(0.045, 1.0);
    let alpha = roughness * roughness;
    let n_dot_v = n.dot(v).max(1e-4);
    let f0 = Vec3::splat(0.04).lerp(s.albedo, s.metallic);
    let diffuse_color = s.albedo * (1.0 - s.metallic);

    // Ambient: Lambert under uniform light gives the albedo itself, plus the
    // analytic environment BRDF for the specular part.
    let (c0, c1) = ([-1.0, -0.0275, -0.572, 0.022], [1.0, 0.0425, 1.04, -0.04]);
    let r: [f32; 4] = std::array::from_fn(|i| roughness * c0[i] + c1[i]);
    let a004 = (r[0] * r[0]).min((-9.28 * n_dot_v).exp2()) * r[0] + r[1];
    let env = f0 * (-1.04 * a004 + r[2]) + Vec3::splat(1.04 * a004 + r[3]);
    let mut color = ambient * (diffuse_color + env);

    let n_dot_l = n.dot(l).max(0.0);
    if n_dot_l > 0.0 {
        let h = (l + v).normalize();
        let (n_dot_h, v_dot_h) = (n.dot(h).max(0.0), v.dot(h).max(0.0));
        let a2 = alpha * alpha;
        let d = {
            let k = n_dot_h * n_dot_h * (a2 - 1.0) + 1.0;
            a2 / (PI * k * k)
        };
        let vis = 0.5
            / (n_dot_l * (n_dot_v * n_dot_v * (1.0 - a2) + a2).sqrt()
                + n_dot_v * (n_dot_l * n_dot_l * (1.0 - a2) + a2).sqrt());
        let fresnel = f0 + (Vec3::ONE - f0) * (1.0 - v_dot_h).powi(5);
        let specular = fresnel * d * vis;
        color += (diffuse_color / PI + specular) * sun * PI * n_dot_l;
    }
    color
}

/// Khronos PBR Neutral, as in forward.wgsl.
pub fn tonemap(c: Vec3) -> Vec3 {
    let start = 0.8 - 0.04;
    let x = c.min_element();
    let offset = if x < 0.08 { x - 6.25 * x * x } else { 0.04 };
    let c = c - Vec3::splat(offset);
    let peak = c.max_element();
    if peak < start {
        return c;
    }
    let d = 1.0 - start;
    let new_peak = 1.0 - d * d / (peak + d - start);
    let c = c * (new_peak / peak);
    let g = 1.0 - 1.0 / (0.15 * (peak - new_peak) + 1.0);
    c.lerp(Vec3::splat(new_peak), g)
}

/// Linear → 8-bit sRGB.
pub fn to_srgb8(c: Vec3) -> [u8; 3] {
    let e = |x: f32| {
        let x = x.clamp(0.0, 1.0);
        let s = if x <= 0.003_130_8 { x * 12.92 } else { 1.055 * x.powf(1.0 / 2.4) - 0.055 };
        (s * 255.0).round() as u8
    };
    [e(c.x), e(c.y), e(c.z)]
}
