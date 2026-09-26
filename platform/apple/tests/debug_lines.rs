//! Viewport ABI v4, v6 and v8, the editor's entries: `fynkel_apple_viewport_overlay`
//! (v6, ADR-0019: lines, triangles and the grid for the next render only),
//! `fynkel_apple_viewport_outline` (v8: the selection outline, likewise), `fynkel_apple_viewport_debug_lines`
//! (the queued segments reach the renderer's debug pass on the next render
//! and only that one; a non-finite segment is dropped; NULL is safe) and
//! `fynkel_apple_viewport_read_frame` (the rendered slot's pixels, RGBA8,
//! equal to `fynkel_render::render_offscreen` of the same extract).

use fynkel_contracts::Entity;
use fynkel_core::{Extract, ExtractRecord, World, WorldConfig};
use fynkel_platform_apple::{
    fynkel_apple_abi_version, fynkel_apple_viewport_debug_lines, fynkel_apple_viewport_outline,
    fynkel_apple_viewport_overlay, fynkel_apple_viewport_read_frame, SyncMode, Viewport,
    APPLE_ABI_VERSION,
};

#[test]
fn debug_lines_draw_on_the_next_render_only() {
    assert_eq!(fynkel_apple_abi_version(), 10);
    assert_eq!(APPLE_ABI_VERSION, 10);

    let mut world = World::new(WorldConfig::default());
    world.begin_frame(1.0 / 60.0);
    world.step();
    let mut vp = Viewport::new(64, 64, 2, SyncMode::MetalCompletion).expect("viewport");

    // Three segments, the middle one with a NaN: two are queued.
    #[rustfmt::skip]
    let lines: [f32; 30] = [
        0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0,
        0.0, 0.0, 0.0, f32::NAN, 1.0, 0.0, 0.0, 1.0, 0.0, 1.0,
        0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 1.0,
    ];
    let queued = unsafe { fynkel_apple_viewport_debug_lines(&mut vp, lines.as_ptr(), 3) };
    assert_eq!(queued, 2);
    let _ = vp.render(&world.extract());
    assert_eq!(
        vp.frame_stats().debug_lines,
        2,
        "drawn by the render that followed"
    );
    let _ = vp.render(&world.extract());
    assert_eq!(
        vp.frame_stats().debug_lines,
        0,
        "the queue empties with every render"
    );

    // Nothing, and NULL.
    assert_eq!(
        unsafe { fynkel_apple_viewport_debug_lines(&mut vp, std::ptr::null(), 0) },
        0
    );
    assert_eq!(
        unsafe { fynkel_apple_viewport_debug_lines(&mut vp, std::ptr::null(), 3) },
        -1
    );
    assert_eq!(
        unsafe { fynkel_apple_viewport_debug_lines(std::ptr::null_mut(), lines.as_ptr(), 3) },
        -1
    );
    vp.ring().wait_idle();
}

#[test]
fn read_frame_returns_the_slots_pixels() {
    let mut world = World::new(WorldConfig::default());
    world.begin_frame(1.0 / 60.0);
    world.step();
    let extract = world.extract();
    let (w, h) = (48u32, 32u32);
    let mut vp = Viewport::new(w, h, 2, SyncMode::MetalCompletion).expect("viewport");
    #[rustfmt::skip]
    let lines: [f32; 10] = [-1.0, 0.0, -2.0, 1.0, 0.0, -2.0, 1.0, 0.0, 0.0, 1.0];
    unsafe { fynkel_apple_viewport_debug_lines(&mut vp, lines.as_ptr(), 1) };
    let slot = vp.render(&extract);
    assert!(slot >= 0);

    let mut out = vec![0u8; (w * h * 4) as usize];
    let n =
        unsafe { fynkel_apple_viewport_read_frame(&vp, slot, out.as_mut_ptr(), out.len() as u32) };
    assert_eq!(n, (w * h * 4) as i32);
    // The same extract through the headless offscreen path `fynkel render`
    // takes, with the same line: the editor's pixel gate in miniature.
    let (device, queue) = fynkel_render::headless_device().expect("gpu");
    let mut renderer =
        fynkel_render::ExtractRenderer::new(&device, wgpu::TextureFormat::Rgba8Unorm, w, h);
    renderer.debug().line(
        glam::Vec3::new(-1.0, 0.0, -2.0),
        glam::Vec3::new(1.0, 0.0, -2.0),
        [1.0, 0.0, 0.0, 1.0],
    );
    let offscreen = fynkel_render::render_offscreen(&device, &queue, &mut renderer, &extract);
    let flat: Vec<u8> = offscreen.iter().flatten().copied().collect();
    assert_eq!(out.len(), flat.len());
    let differing = out
        .chunks_exact(4)
        .zip(flat.chunks_exact(4))
        .filter(|(a, b)| a != b)
        .count();
    assert_eq!(
        differing, 0,
        "viewport and offscreen frames differ in {differing} pixels"
    );
    // The line left pixels (under the default MSAA a one-pixel line is
    // partially covered, so its colour goes through the scene-referred
    // path of the post pass — the count is what is asserted, not red).
    let clear = fynkel_render::clear_rgba8();
    let drawn = out.chunks_exact(4).filter(|p| *p != clear).count();
    assert!(drawn > 10, "the line left {drawn} pixels");

    // Refusals: a slot the ring lacks, a short buffer, NULL.
    assert_eq!(
        unsafe { fynkel_apple_viewport_read_frame(&vp, 7, out.as_mut_ptr(), out.len() as u32) },
        -1
    );
    assert_eq!(
        unsafe { fynkel_apple_viewport_read_frame(&vp, slot, out.as_mut_ptr(), 8) },
        -1
    );
    assert_eq!(
        unsafe { fynkel_apple_viewport_read_frame(&vp, slot, std::ptr::null_mut(), 0) },
        -1
    );
    assert_eq!(
        unsafe { fynkel_apple_viewport_read_frame(std::ptr::null(), slot, out.as_mut_ptr(), 0) },
        -1
    );
    vp.ring().wait_idle();
}

#[test]
fn the_overlay_draws_on_the_next_render_only_and_equals_offscreen() {
    let mut world = World::new(WorldConfig::default());
    world.begin_frame(1.0 / 60.0);
    world.step();
    let extract = world.extract();
    let (w, h) = (48u32, 32u32);
    let mut vp = Viewport::new(w, h, 2, SyncMode::MetalCompletion).expect("viewport");

    // Two lines (one on top, one with a NaN — dropped), a triangle, the grid.
    #[rustfmt::skip]
    let lines: [f32; 36] = [
        -1.0, 0.0, -2.0, 1.0, 0.0, -2.0, 1.0, 0.0, 0.0, 1.0, 3.0, 0.0,
        -1.0, 0.5, -2.0, 1.0, 0.5, -2.0, 0.0, 1.0, 0.0, 1.0, 2.0, 1.0,
        f32::NAN, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 1.0, 0.0,
    ];
    #[rustfmt::skip]
    let triangles: [f32; 14] = [
        -3.0, -3.0, -2.0, 3.0, -3.0, -2.0, 0.0, -0.3, -2.0, 0.0, 0.0, 1.0, 1.0, 1.0,
    ];
    let grid = fynkel_render::Grid::default();
    let mut g = Vec::new();
    for c in [grid.minor, grid.major, grid.x_axis, grid.z_axis] {
        g.extend_from_slice(&c);
    }
    g.extend_from_slice(&[grid.cell, grid.fade]);
    let queued = unsafe {
        fynkel_apple_viewport_overlay(
            &mut vp,
            lines.as_ptr(),
            3,
            triangles.as_ptr(),
            1,
            g.as_ptr(),
        )
    };
    assert_eq!(queued, 3, "two lines and a triangle");
    let slot = vp.render(&extract);
    assert!(slot >= 0);
    let stats = vp.frame_stats();
    assert_eq!(
        (stats.debug_lines, stats.overlay_triangles, stats.grid),
        (2, 1, true)
    );

    let mut out = vec![0u8; (w * h * 4) as usize];
    unsafe { fynkel_apple_viewport_read_frame(&vp, slot, out.as_mut_ptr(), out.len() as u32) };
    // The same through the offscreen path, primitive for primitive.
    let (device, queue) = fynkel_render::headless_device().expect("gpu");
    let mut renderer =
        fynkel_render::ExtractRenderer::new(&device, wgpu::TextureFormat::Rgba8Unorm, w, h);
    let o = renderer.overlay();
    o.line(
        glam::Vec3::new(-1.0, 0.0, -2.0),
        glam::Vec3::new(1.0, 0.0, -2.0),
        [1.0, 0.0, 0.0, 1.0],
        3.0,
    );
    o.line_on_top(
        glam::Vec3::new(-1.0, 0.5, -2.0),
        glam::Vec3::new(1.0, 0.5, -2.0),
        [0.0, 1.0, 0.0, 1.0],
        2.0,
    );
    o.triangle_on_top(
        glam::Vec3::new(-3.0, -3.0, -2.0),
        glam::Vec3::new(3.0, -3.0, -2.0),
        glam::Vec3::new(0.0, -0.3, -2.0),
        [0.0, 0.0, 1.0, 1.0],
    );
    o.grid(grid);
    let offscreen = fynkel_render::render_offscreen(&device, &queue, &mut renderer, &extract);
    let flat: Vec<u8> = offscreen.iter().flatten().copied().collect();
    let differing = out
        .chunks_exact(4)
        .zip(flat.chunks_exact(4))
        .filter(|(a, b)| a != b)
        .count();
    assert_eq!(
        differing, 0,
        "viewport and offscreen differ in {differing} pixels"
    );
    // Pure red, green and blue made it to the frame.
    for rgb in [[255, 0, 0], [0, 255, 0], [0, 0, 255]] {
        assert!(out.chunks_exact(4).any(|p| p[..3] == rgb), "{rgb:?}");
    }

    let _ = vp.render(&extract);
    let stats = vp.frame_stats();
    assert_eq!(
        (stats.debug_lines, stats.overlay_triangles, stats.grid),
        (0, 0, false)
    );
    // NULL arrays with counts, a NULL viewport.
    unsafe {
        assert_eq!(
            fynkel_apple_viewport_overlay(
                &mut vp,
                std::ptr::null(),
                0,
                std::ptr::null(),
                0,
                std::ptr::null()
            ),
            0
        );
        assert_eq!(
            fynkel_apple_viewport_overlay(
                &mut vp,
                std::ptr::null(),
                1,
                std::ptr::null(),
                0,
                std::ptr::null()
            ),
            -1
        );
        assert_eq!(
            fynkel_apple_viewport_overlay(
                &mut vp,
                std::ptr::null(),
                0,
                std::ptr::null(),
                2,
                std::ptr::null()
            ),
            -1
        );
        assert_eq!(
            fynkel_apple_viewport_overlay(
                std::ptr::null_mut(),
                lines.as_ptr(),
                1,
                std::ptr::null(),
                0,
                std::ptr::null()
            ),
            -1
        );
    }
    vp.ring().wait_idle();
}

/// Two placeholder cubes in front of the default camera: the selected one
/// (bits 1) and its child (bits 2).
fn two_cubes() -> Extract {
    let mut e = Extract::default();
    for (bits, x) in [(1u64, -0.7f32), (2, 0.7)] {
        e.records.push(ExtractRecord {
            entity: Entity::from_bits(bits),
            world: glam::Mat4::from_translation(glam::Vec3::new(x, 0.0, 0.0)),
            overridden: false,
            layer_bit: 1,
        });
    }
    e
}

#[test]
fn the_outline_draws_on_the_next_render_only_and_equals_offscreen() {
    let extract = two_cubes();
    let (w, h) = (64u32, 48u32);
    let mut vp = Viewport::new(w, h, 2, SyncMode::MetalCompletion).expect("viewport");
    let (selected, children) = ([1u64], [2u64, 1]);
    // Selected red, children green, three pixels.
    #[rustfmt::skip]
    let style: [f32; 9] = [1.0, 0.0, 0.0, 1.0, 0.0, 1.0, 0.0, 1.0, 3.0];
    let queued = unsafe {
        fynkel_apple_viewport_outline(
            &mut vp,
            selected.as_ptr(),
            1,
            children.as_ptr(),
            2,
            style.as_ptr(),
        )
    };
    assert_eq!(queued, 3);
    let slot = vp.render(&extract);
    assert_eq!(vp.frame_stats().outlined, 2);
    let mut out = vec![0u8; (w * h * 4) as usize];
    unsafe { fynkel_apple_viewport_read_frame(&vp, slot, out.as_mut_ptr(), out.len() as u32) };

    // The same through the offscreen path.
    let (device, queue) = fynkel_render::headless_device().expect("gpu");
    let mut renderer =
        fynkel_render::ExtractRenderer::new(&device, wgpu::TextureFormat::Rgba8Unorm, w, h);
    renderer.overlay().outline(
        &[Entity::from_bits(1)],
        &[Entity::from_bits(2), Entity::from_bits(1)],
        fynkel_render::OutlineStyle {
            selected: [1.0, 0.0, 0.0, 1.0],
            children: [0.0, 1.0, 0.0, 1.0],
            width: 3.0,
        },
    );
    let offscreen = fynkel_render::render_offscreen(&device, &queue, &mut renderer, &extract);
    let flat: Vec<u8> = offscreen.iter().flatten().copied().collect();
    let differing = out
        .chunks_exact(4)
        .zip(flat.chunks_exact(4))
        .filter(|(a, b)| a != b)
        .count();
    assert_eq!(
        differing, 0,
        "viewport and offscreen differ in {differing} pixels"
    );
    // Both rings made it to the frame.
    for rgb in [[255, 0, 0], [0, 255, 0]] {
        assert!(out.chunks_exact(4).any(|p| p[..3] == rgb), "{rgb:?}");
    }

    // The next render has none: neither ring is left. (Only first frames
    // are compared pixel for pixel — a viewport's later frames carry its
    // temporal state, which a fresh offscreen renderer does not have.)
    let slot = vp.render(&extract);
    assert_eq!(vp.frame_stats().outlined, 0);
    unsafe { fynkel_apple_viewport_read_frame(&vp, slot, out.as_mut_ptr(), out.len() as u32) };
    for rgb in [[255, 0, 0], [0, 255, 0]] {
        assert!(
            !out.chunks_exact(4).any(|p| p[..3] == rgb),
            "{rgb:?} left over"
        );
    }

    // The default style, NULL lists with counts, a non-finite style, a
    // NULL viewport.
    unsafe {
        assert_eq!(
            fynkel_apple_viewport_outline(
                &mut vp,
                selected.as_ptr(),
                1,
                std::ptr::null(),
                0,
                std::ptr::null()
            ),
            1
        );
        assert_eq!(
            fynkel_apple_viewport_outline(
                &mut vp,
                std::ptr::null(),
                1,
                std::ptr::null(),
                0,
                std::ptr::null()
            ),
            -1
        );
        assert_eq!(
            fynkel_apple_viewport_outline(
                &mut vp,
                std::ptr::null(),
                0,
                std::ptr::null(),
                1,
                std::ptr::null()
            ),
            -1
        );
        let mut bad = style;
        bad[8] = f32::NAN;
        assert_eq!(
            fynkel_apple_viewport_outline(
                &mut vp,
                selected.as_ptr(),
                1,
                std::ptr::null(),
                0,
                bad.as_ptr()
            ),
            -1
        );
        assert_eq!(
            fynkel_apple_viewport_outline(
                std::ptr::null_mut(),
                selected.as_ptr(),
                1,
                std::ptr::null(),
                0,
                std::ptr::null()
            ),
            -1
        );
    }
    // The default-style outline queued above is drawn.
    let _ = vp.render(&extract);
    assert_eq!(vp.frame_stats().outlined, 1);
    vp.ring().wait_idle();
}
