//! Compare the tiled GPU kernel with the scalar depth-aware upsampling
//! filter over a half-resolution estimate, including partial workgroups,
//! screen edges and depth discontinuities.
use super::*;
use bytemuck::Zeroable;

#[test]
fn tiled_blur_matches_scalar_filter_at_edges_and_depth_breaks() {
    let Some((device, queue)) = headless_device() else {
        return;
    };
    let frame_layout = create_ao_frame_layout(&device);
    let inputs_layout = create_ao_blur_layout(&device, 4);
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: None,
        source: wgpu::ShaderSource::Wgsl(
            format!(
                "{}{}",
                include_str!("shaders/frame.wgsl"),
                include_str!("shaders/ao_blur.wgsl")
                    .replace("DEPTH_TEXTURE", "texture_2d<f32>")
                    .replace("DEPTH_LOAD", "textureLoad(depth_tex, c, 0).x")
            )
            .into(),
        ),
    });
    let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: None,
        bind_group_layouts: &[Some(&frame_layout), Some(&inputs_layout)],
        ..Default::default()
    });
    let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: None,
        layout: Some(&layout),
        module: &shader,
        entry_point: Some("cs_main"),
        compilation_options: Default::default(),
        cache: None,
    });
    // Real projection matrices exercise the perspective divide used by the
    // bilateral depth threshold, rather than only an identity reconstruction.
    let eye = glam::Vec3::new(2.0, 3.0, 7.0);
    let view = glam::camera::rh::view::look_at_mat4(eye, glam::Vec3::ZERO, glam::Vec3::Y);
    for (projection, inverse, eye) in [
        (
            "identity",
            glam::Mat4::IDENTITY,
            glam::Vec3::new(0.0, 0.0, 3.0),
        ),
        (
            "perspective",
            (glam::camera::rh::proj::directx::perspective(1.0, 1.7, 0.1, 100.0) * view).inverse(),
            eye,
        ),
        (
            "orthographic",
            (glam::camera::rh::proj::directx::orthographic(-4.0, 4.0, -3.0, 3.0, 0.1, 100.0)
                * view)
                .inverse(),
            eye,
        ),
    ] {
        let mut uniforms = FrameUniforms::zeroed();
        uniforms.inv_view_proj = inverse.to_cols_array_2d();
        uniforms.camera_pos = eye.extend(0.0).to_array();
        let uniform = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: None,
            contents: bytemuck::bytes_of(&uniforms),
            usage: wgpu::BufferUsages::UNIFORM,
        });
        let frame = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &frame_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform.as_entire_binding(),
            }],
        });
        for (width, height, pattern) in [
            (1u32, 1u32),
            (1, 17),
            (17, 1),
            (2, 2),
            (3, 3),
            (3, 4),
            (4, 3),
            (4, 4),
            (5, 5),
            (7, 3),
            (8, 8),
            (9, 17),
            (15, 17),
            (16, 1),
            (16, 16),
            (31, 33),
            (17, 15),
            (23, 19),
            (33, 31),
        ]
        .into_iter()
        .map(|(w, h)| (w, h, 0))
        .chain([
            (1, 1, 1),
            (17, 15, 1),
            (33, 31, 1),
            (49, 31, 2),
            (49, 31, 3),
        ]) {
            let extent = wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            };
            let (half_w, half_h) = (width.div_ceil(2), height.div_ceil(2));
            let half_extent = wgpu::Extent3d {
                width: half_w,
                height: half_h,
                depth_or_array_layers: 1,
            };
            let make_sized = |size, format, usage| {
                device.create_texture(&wgpu::TextureDescriptor {
                    label: None,
                    size,
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format,
                    usage,
                    view_formats: &[],
                })
            };
            let make = |format, usage| {
                device.create_texture(&wgpu::TextureDescriptor {
                    label: None,
                    size: extent,
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format,
                    usage,
                    view_formats: &[],
                })
            };
            let input_usage = wgpu::TextureUsages::COPY_DST | wgpu::TextureUsages::TEXTURE_BINDING;
            let depth = make(wgpu::TextureFormat::R32Float, input_usage);
            let ao = make_sized(half_extent, wgpu::TextureFormat::Rg8Unorm, input_usage);
            let output = make(
                wgpu::TextureFormat::Rgba8Unorm,
                wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::COPY_SRC,
            );
            let depths: Vec<f32> = (0..width * height)
                .map(|i| if i % width < width / 2 { 0.2 } else { 0.8 })
                .collect();
            let colours: Vec<[u8; 2]> = (0..half_w * half_h)
                .map(|i| match pattern {
                    0 => [(i * 37 % 256) as u8, (i * 71 % 256) as u8],
                    1 => [255, 255],
                    // Isolated AO/contact taps on tile boundaries and at the
                    // screen edge must prevent adjacent halo tiles skipping.
                    _ if i == half_w * 4 + 4
                        || i == half_w * 8 + 12
                        || i == half_w * half_h - 1 =>
                    {
                        if pattern == 2 {
                            [0, 255]
                        } else {
                            [255, 0]
                        }
                    }
                    _ => [255, 255],
                })
                .collect();
            for (texture, bytes, stride, size) in [
                (&depth, bytemuck::cast_slice(&depths), width * 4, extent),
                (&ao, bytemuck::cast_slice(&colours), half_w * 2, half_extent),
            ] {
                queue.write_texture(
                    wgpu::TexelCopyTextureInfo {
                        texture,
                        mip_level: 0,
                        origin: wgpu::Origin3d::ZERO,
                        aspect: wgpu::TextureAspect::All,
                    },
                    bytes,
                    wgpu::TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(stride),
                        rows_per_image: Some(size.height),
                    },
                    size,
                );
            }
            let views = [
                depth.create_view(&Default::default()),
                ao.create_view(&Default::default()),
                output.create_view(&Default::default()),
            ];
            let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: None,
                layout: &inputs_layout,
                entries: &std::array::from_fn::<_, 3, _>(|i| wgpu::BindGroupEntry {
                    binding: i as u32,
                    resource: wgpu::BindingResource::TextureView(&views[i]),
                }),
            });
            let buffer = device.create_buffer(&wgpu::BufferDescriptor {
                label: None,
                size: u64::from(height) * 256,
                usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                mapped_at_creation: false,
            });
            let mut encoder = device.create_command_encoder(&Default::default());
            {
                let mut pass = encoder.begin_compute_pass(&Default::default());
                pass.set_pipeline(&pipeline);
                pass.set_bind_group(0, &frame, &[]);
                pass.set_bind_group(1, &group, &[]);
                pass.dispatch_workgroups(width.div_ceil(8), height.div_ceil(8), 1);
            }
            encoder.copy_texture_to_buffer(
                wgpu::TexelCopyTextureInfo {
                    texture: &output,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                wgpu::TexelCopyBufferInfo {
                    buffer: &buffer,
                    layout: wgpu::TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(256),
                        rows_per_image: Some(height),
                    },
                },
                extent,
            );
            queue.submit([encoder.finish()]);
            let (tx, rx) = std::sync::mpsc::channel();
            buffer
                .slice(..)
                .map_async(wgpu::MapMode::Read, move |r| tx.send(r).unwrap());
            device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
            rx.recv().unwrap().unwrap();
            let data = buffer.slice(..).get_mapped_range().unwrap();
            let radial = |x: u32, y: u32| {
                let point = glam::Vec3::new(
                    (x as f32 + 0.5) / width as f32 * 2.0 - 1.0,
                    1.0 - (y as f32 + 0.5) / height as f32 * 2.0,
                    depths[(y * width + x) as usize],
                );
                inverse.project_point3(point).distance(eye)
            };
            // What a half-resolution texel stands for: the full-size depth
            // texel at its top-left.
            let rep = |hx: u32, hy: u32| radial((hx * 2).min(width - 1), (hy * 2).min(height - 1));
            for y in 0..height {
                for x in 0..width {
                    let center = radial(x, y);
                    let mut sum = [0.0f32; 2];
                    let mut count = 0.0;
                    // The two half-resolution texels around each axis.
                    let (fx, fy) = ((x as i32 - 1) >> 1, (y as i32 - 1) >> 1);
                    for hy in fy..fy + 2 {
                        for hx in fx..fx + 2 {
                            if hx < 0 || hy < 0 || hx >= half_w as i32 || hy >= half_h as i32 {
                                continue;
                            }
                            let (hx, hy) = (hx as u32, hy as u32);
                            if (rep(hx, hy) - center).abs() > 0.02 * center.max(1.0) {
                                continue;
                            }
                            let colour = colours[(hy * half_w + hx) as usize];
                            for c in 0..2 {
                                sum[c] += f32::from(colour[c]) / 255.0;
                            }
                            count += 1.0;
                        }
                    }
                    // None on this surface: the kernel takes the nearest in
                    // depth, which ties make ambiguous; not compared here.
                    if count == 0.0 {
                        continue;
                    }
                    for c in 0..2 {
                        let expected = (sum[c] / count * 255.0).round() as i32;
                        let actual = i32::from(data[(y * 256 + x * 4 + c as u32) as usize]);
                        if pattern == 1 {
                            assert_eq!(actual, 255, "white tile must remain exactly white");
                        }
                        assert!(
                        (actual - expected).abs() <= 1,
                        "{projection} {width}x{height} ({x},{y}) channel {c}: {actual} vs {expected}"
                    );
                    }
                    assert_eq!(
                        &data[(y * 256 + x * 4 + 2) as usize..(y * 256 + x * 4 + 4) as usize],
                        &[0, 255]
                    );
                }
            }
            drop(data);
            buffer.unmap();
        }
    }
}
