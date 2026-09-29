//! Image-based lighting: the environment cube, its prefiltered mips and the
//! split-sum BRDF lookup table.

use super::*;

/// The cooked environment on the GPU (E1): the cosine-convolved
/// irradiance cube (E/π) and the GGX-prefiltered specular cube, whose mip
/// levels run roughness 0 → 1. Both are `Rgba16Float`, both sampled with
/// the renderer's clamped trilinear `cube_sampler`.
pub(crate) struct Ibl {
    pub(crate) irradiance: GpuTexture,
    pub(crate) specular: GpuTexture,
    pub(crate) specular_mips: u32,
}

impl Ibl {
    /// What a scene without an HDR environment binds: black 1×1 cubes, so
    /// group 0 is always complete and the shader needs no variant.
    /// wgpu zeroes a fresh texture, so there is nothing to write.
    pub(crate) fn none(device: &wgpu::Device) -> Ibl {
        Ibl {
            irradiance: create_cube(device, 1, 1, "fynkel ibl none").0,
            specular: create_cube(device, 1, 1, "fynkel ibl none").0,
            specular_mips: 1,
        }
    }

    /// Uploads a cooked `ENVM` payload.
    pub(crate) fn upload(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        env: &EnvironmentPayload,
        r: &AssetRef,
    ) -> Ibl {
        let halves = |v: &[u16]| -> Vec<u8> {
            let mut out = Vec::with_capacity(v.len() * 2);
            for h in v {
                out.extend_from_slice(&h.to_le_bytes());
            }
            out
        };
        let irradiance_bytes = halves(&env.irradiance);
        let face = (env.irradiance_size * env.irradiance_size) as usize * 8;
        let irradiance_faces: Vec<&[u8]> = (0..6)
            .map(|f| &irradiance_bytes[f * face..(f + 1) * face])
            .collect();
        let irradiance = upload_cube(
            device,
            queue,
            env.irradiance_size,
            1,
            &irradiance_faces,
            &format!("fynkel irradiance {r}"),
        );
        // Every specular level, face by face (the payload is mip-major).
        let mut specular_bytes = Vec::with_capacity(env.specular_mips as usize);
        for mip in 0..env.specular_mips {
            specular_bytes.push(halves(env.specular_level(mip).unwrap_or(&[])));
        }
        let mut specular_faces: Vec<&[u8]> = Vec::with_capacity(env.specular_mips as usize * 6);
        for (mip, bytes) in specular_bytes.iter().enumerate() {
            let size = (env.specular_size >> mip).max(1) as usize;
            let face = size * size * 8;
            for f in 0..6 {
                specular_faces.push(&bytes[f * face..(f + 1) * face]);
            }
        }
        let specular = upload_cube(
            device,
            queue,
            env.specular_size,
            env.specular_mips,
            &specular_faces,
            &format!("fynkel environment {r}"),
        );
        Ibl {
            irradiance,
            specular,
            specular_mips: env.specular_mips,
        }
    }

    pub(crate) fn bytes(&self) -> u64 {
        self.irradiance.bytes + self.specular.bytes
    }
}

/// Uploads a `Rgba16Float` cube map. `faces` is mip-major, face-minor
/// (the `ENVM` order): level 0's six faces, then level 1's, …
pub(crate) fn upload_cube(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    size: u32,
    mips: u32,
    faces: &[&[u8]],
    label: &str,
) -> GpuTexture {
    let (mut gpu, texture) = create_cube(device, size, mips, label);
    let mut bytes = 0u64;
    for (i, data) in faces.iter().enumerate() {
        let (mip, face) = (i as u32 / 6, i as u32 % 6);
        let side = (size >> mip).max(1);
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: mip,
                origin: wgpu::Origin3d {
                    x: 0,
                    y: 0,
                    z: face,
                },
                aspect: wgpu::TextureAspect::All,
            },
            data,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(side * 8),
                rows_per_image: Some(side),
            },
            wgpu::Extent3d {
                width: side,
                height: side,
                depth_or_array_layers: 1,
            },
        );
        bytes += data.len() as u64;
    }
    gpu.bytes = bytes;
    gpu
}

/// An empty `Rgba16Float` cube (zeroed by wgpu) and its texture, so a
/// caller can write the levels later.
pub(crate) fn create_cube(
    device: &wgpu::Device,
    size: u32,
    mips: u32,
    label: &str,
) -> (GpuTexture, wgpu::Texture) {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d {
            width: size,
            height: size,
            depth_or_array_layers: 6,
        },
        mip_level_count: mips,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba16Float,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    let view = texture.create_view(&wgpu::TextureViewDescriptor {
        dimension: Some(wgpu::TextureViewDimension::Cube),
        ..Default::default()
    });
    (GpuTexture { view, bytes: 0 }, texture)
}

/// The split-sum environment BRDF (E1): `∫ f(l, v) / F · n·l dl` over the
/// GGX lobe, integrated once on the CPU — `x` = `n·v`, `y` = roughness,
/// `r` = the scale on `F0`, `g` = the bias. The specular answer for a
/// prefiltered radiance `L` is `L · (F0 · r + g)`. 64×64 RGBA16F; the
/// table depends on nothing but the BRDF, so it is computed once per
/// process and uploaded per renderer.
pub(crate) fn create_brdf_lut(device: &wgpu::Device) -> (GpuTexture, wgpu::Texture) {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("fynkel brdf lut"),
        size: wgpu::Extent3d {
            width: BRDF_LUT_SIZE,
            height: BRDF_LUT_SIZE,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba16Float,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    let view = texture.create_view(&Default::default());
    (
        GpuTexture {
            view,
            bytes: (BRDF_LUT_SIZE * BRDF_LUT_SIZE * 8) as u64,
        },
        texture,
    )
}

/// Fills the table on the first frame (`new` has no queue); the texels
/// are integrated once per process.
pub(crate) fn write_brdf_lut(queue: &wgpu::Queue, texture: &wgpu::Texture) {
    static LUT: std::sync::OnceLock<Vec<u8>> = std::sync::OnceLock::new();
    queue.write_texture(
        wgpu::TexelCopyTextureInfo {
            texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        LUT.get_or_init(brdf_lut_texels),
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(BRDF_LUT_SIZE * 8),
            rows_per_image: Some(BRDF_LUT_SIZE),
        },
        wgpu::Extent3d {
            width: BRDF_LUT_SIZE,
            height: BRDF_LUT_SIZE,
            depth_or_array_layers: 1,
        },
    );
}

pub(crate) const BRDF_LUT_SIZE: u32 = 64;

pub(crate) const BRDF_LUT_SAMPLES: u32 = 512;

pub(crate) fn brdf_lut_texels() -> Vec<u8> {
    use fynkel_cooked::payload::f32_to_f16;
    let n = BRDF_LUT_SIZE as usize;
    let mut out = Vec::with_capacity(n * n * 8);
    for y in 0..n {
        let roughness = (y as f32 + 0.5) / n as f32;
        let a = roughness * roughness;
        let a2 = a * a;
        for x in 0..n {
            let ndotv = ((x as f32 + 0.5) / n as f32).max(1e-3);
            let v = [(1.0 - ndotv * ndotv).max(0.0).sqrt(), 0.0, ndotv];
            let (mut scale, mut bias) = (0.0f32, 0.0f32);
            for i in 0..BRDF_LUT_SAMPLES {
                let (x1, x2) = hammersley(i, BRDF_LUT_SAMPLES);
                let phi = 2.0 * std::f32::consts::PI * x1;
                let cos_theta = ((1.0 - x2) / (1.0 + (a2 - 1.0) * x2)).sqrt();
                let sin_theta = (1.0 - cos_theta * cos_theta).max(0.0).sqrt();
                // The half vector in the tangent frame around +Z = N.
                let h = [sin_theta * phi.cos(), sin_theta * phi.sin(), cos_theta];
                let vh = v[0] * h[0] + v[1] * h[1] + v[2] * h[2];
                let l = [
                    2.0 * vh * h[0] - v[0],
                    2.0 * vh * h[1] - v[1],
                    2.0 * vh * h[2] - v[2],
                ];
                if l[2] <= 0.0 {
                    continue;
                }
                let (ndotl, ndoth, vdoth) = (l[2], h[2].max(0.0), vh.max(0.0));
                // Height-correlated Smith visibility, the shader's.
                let gv = ndotl * (ndotv * ndotv * (1.0 - a2) + a2).sqrt();
                let gl = ndotv * (ndotl * ndotl * (1.0 - a2) + a2).sqrt();
                let vis = 0.5 / (gv + gl).max(1e-6);
                // pdf = D · n·h / (4 · v·h); the D cancels.
                let weight = 4.0 * vis * ndotl * vdoth / ndoth.max(1e-6);
                let fc = (1.0 - vdoth).powi(5);
                scale += (1.0 - fc) * weight;
                bias += fc * weight;
            }
            let (scale, bias) = (
                scale / BRDF_LUT_SAMPLES as f32,
                bias / BRDF_LUT_SAMPLES as f32,
            );
            for v in [scale, bias, 0.0, 1.0] {
                out.extend_from_slice(&f32_to_f16(v).to_le_bytes());
            }
        }
    }
    out
}

/// The Hammersley point set (the cook's, `fynkel_import::environment`).
pub(crate) fn hammersley(i: u32, n: u32) -> (f32, f32) {
    let mut bits = i;
    bits = bits.rotate_right(16);
    bits = ((bits & 0x5555_5555) << 1) | ((bits & 0xAAAA_AAAA) >> 1);
    bits = ((bits & 0x3333_3333) << 2) | ((bits & 0xCCCC_CCCC) >> 2);
    bits = ((bits & 0x0F0F_0F0F) << 4) | ((bits & 0xF0F0_F0F0) >> 4);
    bits = ((bits & 0x00FF_00FF) << 8) | ((bits & 0xFF00_FF00) >> 8);
    (i as f32 / n as f32, bits as f32 * 2.328_306_4e-10)
}
