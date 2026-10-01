//! Viewport ABI 11 (foundation 4.3): a panic inside a viewport call never
//! crosses the C boundary (Rust aborts the process when one does, taking
//! the host app with it). The call returns its failure value, the viewport
//! is broken from then on — every call fails, `render` with
//! `FYNKEL_APPLE_RENDER_BROKEN` — and keeps the panic's message for the
//! host to show; destroying it still works. The world it drew is untouched.

use std::ptr;

use fynkel_native::{
    fynkel_runtime_create, fynkel_runtime_destroy, fynkel_world_create, fynkel_world_destroy,
    fynkel_world_panic_message,
};
use fynkel_platform_apple::{
    fynkel_apple_viewport_acquire_presented, fynkel_apple_viewport_create,
    fynkel_apple_viewport_debug_panic, fynkel_apple_viewport_destroy, fynkel_apple_viewport_error,
    fynkel_apple_viewport_render, fynkel_apple_viewport_resize,
    fynkel_apple_viewport_set_frame_ready_callback, FYNKEL_APPLE_RENDER_BROKEN,
};

fn error(vp: *const fynkel_platform_apple::Viewport) -> String {
    let mut out = [0u8; 256];
    let n = unsafe { fynkel_apple_viewport_error(vp, out.as_mut_ptr(), out.len() as u32) };
    assert!(n >= 0 && (n as usize) <= out.len(), "{n}");
    String::from_utf8(out[..n as usize].to_vec()).unwrap()
}

#[test]
fn a_panic_breaks_the_viewport_not_the_process() {
    let rt = fynkel_runtime_create();
    let world = unsafe { fynkel_world_create(rt, ptr::null(), std::ptr::null_mut()) };
    let vp = fynkel_apple_viewport_create(32, 32, 3, 1);
    assert!(!vp.is_null());
    unsafe {
        assert!(fynkel_apple_viewport_render(vp, world) >= 0);
        assert_eq!(error(vp), "", "a working viewport has no error");

        assert_eq!(fynkel_apple_viewport_debug_panic(vp), -1);
        assert!(
            error(vp).contains("fynkel_apple_viewport_debug_panic"),
            "{}",
            error(vp)
        );

        assert_eq!(
            fynkel_apple_viewport_render(vp, world),
            FYNKEL_APPLE_RENDER_BROKEN
        );
        assert_eq!(fynkel_apple_viewport_resize(vp, 64, 64), -1);
        assert_eq!(fynkel_apple_viewport_acquire_presented(vp), -1);
        // Detaching the presenter must still work on a broken viewport.
        fynkel_apple_viewport_set_frame_ready_callback(vp, None, ptr::null_mut());
        // A second panic keeps the first message.
        assert_eq!(fynkel_apple_viewport_debug_panic(vp), -1);
        assert!(error(vp).contains("fynkel_apple_viewport_debug_panic"));
        // A short buffer gets the start; the call answers the bytes written.
        let mut short = [0u8; 4];
        assert_eq!(fynkel_apple_viewport_error(vp, short.as_mut_ptr(), 4), 4);
        assert_eq!(
            fynkel_apple_viewport_error(ptr::null(), short.as_mut_ptr(), 4),
            0
        );

        // The world was not involved.
        assert_eq!(fynkel_world_panic_message(world).length, 0);

        fynkel_apple_viewport_destroy(vp);
        fynkel_world_destroy(world);
        fynkel_runtime_destroy(rt);
    }
}
