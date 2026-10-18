//! Stage 10: materials' own shaders through the C interface, as
//! `package:fynkel` uses them: one that doesn't compile says where, one
//! that does says what it takes, and its values, an object's own numbers,
//! the time and a new version of its code all show in the next frame.
#![allow(unsafe_code)]

mod common;

use common::*;
use fynkel_ffi::*;

const TINT: &str = "struct Values { tint: vec4<f32>, steps: i32 }
struct PerObject { boost: f32 }
var picture: color_texture;

fn unlit(in: SurfaceInput) -> vec4<f32> {
    return vec4<f32>(values.tint.rgb + per_object.boost + step(1.0, frame_time()) * vec3<f32>(0.0, 0.0, 1.0), 1.0);
}
";

/// A compiled shader that frees itself.
#[derive(Debug)]
struct Compiled(*mut FyShader);

impl Drop for Compiled {
    fn drop(&mut self) {
        // SAFETY: made by fy_shader_new and freed only here.
        unsafe { fy_shader_free(self.0) };
    }
}

fn compile(name: &str, source: &str) -> Result<Compiled, String> {
    let mut shader = std::ptr::null_mut();
    // SAFETY: slices of the lengths passed and a local out-value.
    let status = unsafe { fy_shader_new(name.as_ptr(), name.len(), source.as_ptr(), source.len(), &mut shader) };
    if status == FyStatus::Ok { Ok(Compiled(shader)) } else { Err(last_error()) }
}

/// `source` compiled and added to `world`; the handle's freed, the world
/// keeping its share.
fn add(world: &World, name: &str, source: &str) -> Result<u64, String> {
    let shader = compile(name, source)?;
    let mut id = 0;
    // SAFETY: a live world and shader, a local out-value.
    ok(unsafe { fy_shader_add(world.0, shader.0, &mut id) });
    Ok(id)
}

fn describe(shader: &Compiled) -> String {
    let mut len = 0;
    // SAFETY: a live shader and a local out-value.
    ok(unsafe { fy_shader_describe(shader.0, std::ptr::null_mut(), 0, &mut len) });
    let mut text = vec![0u8; len];
    // SAFETY: a live shader and a buffer of the length passed.
    ok(unsafe { fy_shader_describe(shader.0, text.as_mut_ptr(), text.len(), &mut len) });
    String::from_utf8(text).unwrap()
}

#[test]
fn a_shader_that_doesnt_compile_says_where_in_its_own_file() {
    let error = compile("broken.wgsl", "fn surface(in: SurfaceInput) -> Surface {\n    return 1.0;\n}\n").unwrap_err();
    assert!(error.starts_with("broken.wgsl:2:"), "{error}");
}

#[test]
fn a_shader_says_what_it_takes() {
    assert_eq!(
        describe(&compile("tint.wgsl", TINT).unwrap()),
        "unlit\nvalues 32\nvalue tint f32 4 0\nvalue steps i32 1 16\ntexture picture srgb\nown boost\n"
    );
}

/// The middle of the view.
fn centre(view: &View) -> [u8; 4] {
    view.read(view.frame(), 32, 32).pixel(16, 16)
}

#[test]
fn values_own_numbers_time_and_new_code_show_in_the_next_frame() {
    let world = World::new();
    let cube = stage1(&world);
    let shader = add(&world, "tint.wgsl", TINT).unwrap();
    let (mut renderer, mut material) = (FyMeshRenderer::default(), FyMaterial::default());
    // SAFETY: a live world, pointers to locals and slices of the lengths passed.
    unsafe {
        ok(fy_mesh_renderer_get(world.0, cube, &mut renderer));
        ok(fy_material_get(world.0, renderer.material, &mut material));
        ok(fy_material_set(world.0, renderer.material, &FyMaterial { shader, ..material }));
        let red: Vec<u8> = [1.0f32, 0.0, 0.0, 1.0].iter().flat_map(|v| v.to_le_bytes()).collect();
        ok(fy_material_values_set(world.0, renderer.material, 0, red.as_ptr(), red.len()));
        assert_eq!(fy_material_values_set(world.0, renderer.material, 30, red.as_ptr(), 4), FyStatus::WrongSize);
        assert_eq!(fy_shader_remove(world.0, shader), FyStatus::InUse);
    }
    let view = View::headless(&world, 32, 32);
    let [r, g, b, _] = centre(&view);
    assert!(r > 200 && g < 40 && b < 40, "red: {r} {g} {b}");

    // SAFETY: a live world and a local value.
    ok(unsafe { fy_mesh_renderer_set(world.0, cube, &FyMeshRenderer { own: [1.0, 0.0, 0.0, 0.0], ..renderer }) });
    let [r, g, b, _] = centre(&view);
    assert!(r > 240 && g > 150 && b > 150, "boosted: {r} {g} {b}");

    // SAFETY: live world and values.
    unsafe {
        ok(fy_mesh_renderer_set(world.0, cube, &renderer));
        ok(fy_world_time_set(world.0, 2.0, 0.016));
        assert_eq!(fy_world_time_set(world.0, f32::NAN, 0.0), FyStatus::InvalidValue);
    }
    let [r, g, b, _] = centre(&view);
    assert!(r > 200 && g < 40 && b > 200, "time: {r} {g} {b}");

    // New code shows, though its handle's freed.
    let green =
        "struct Values { tint: vec4<f32> }\nfn unlit(in: SurfaceInput) -> vec4<f32> {\n    return vec4<f32>(values.tint.g, 1.0, 0.0, 1.0);\n}\n";
    let new = compile("tint.wgsl", green).unwrap();
    // SAFETY: a live world and shader.
    ok(unsafe { fy_shader_set(world.0, shader, new.0) });
    drop(new);
    let [r, g, b, _] = centre(&view);
    assert!(r < 40 && g > 200 && b < 40, "new code: {r} {g} {b}");

    // SAFETY: a live world and pointers to locals.
    unsafe {
        ok(fy_material_set(world.0, renderer.material, &material));
        ok(fy_shader_remove(world.0, shader));
        assert_eq!(fy_shader_remove(world.0, shader), FyStatus::NoShader);
    }
}
