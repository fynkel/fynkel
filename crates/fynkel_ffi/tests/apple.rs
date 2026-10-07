//! The Apple view, through the C interface, the way the Swift plugin uses
//! it: `fy_apple_set_frame_ready` for the signal, `fy_apple_acquire` for
//! the IOSurface, whose pixels are read with IOSurface's own C functions.
#![cfg(target_vendor = "apple")]
#![allow(unsafe_code)]

mod common;

use std::ffi::c_void;
use std::ptr;
use std::sync::mpsc;
use std::time::Duration;

use common::*;
use fynkel_ffi::*;
use fynkel_golden::{Image, assert_golden};

#[link(name = "IOSurface", kind = "framework")]
unsafe extern "C" {
    fn IOSurfaceLock(surface: *mut c_void, options: u32, seed: *mut u32) -> i32;
    fn IOSurfaceUnlock(surface: *mut c_void, options: u32, seed: *mut u32) -> i32;
    fn IOSurfaceGetBaseAddress(surface: *mut c_void) -> *const u8;
    fn IOSurfaceGetBytesPerRow(surface: *mut c_void) -> usize;
    fn IOSurfaceGetWidth(surface: *mut c_void) -> usize;
    fn IOSurfaceGetHeight(surface: *mut c_void) -> usize;
}

#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    fn CFRelease(object: *mut c_void);
    fn CFGetRetainCount(object: *mut c_void) -> isize;
}

const READ_ONLY: u32 = 1;

/// An Apple view whose frame-ready signals arrive on a channel.
struct AppleView {
    view: *mut FyView,
    ready: mpsc::Receiver<()>,
    sender: Box<mpsc::SyncSender<()>>,
}

unsafe extern "C" fn frame_ready(userdata: *mut c_void) {
    // SAFETY: the userdata is the view's boxed sender, which outlives the
    // callback (Drop clears it first).
    let sender = unsafe { &*userdata.cast::<mpsc::SyncSender<()>>() };
    let _ = sender.try_send(());
}

impl AppleView {
    fn new(world: &World, width: u32, height: u32) -> Self {
        let mut view = ptr::null_mut();
        // SAFETY: a live world and a local out-value.
        ok(unsafe { fy_view_new_apple(world.0, width, height, &mut view) });
        let (sender, ready) = mpsc::sync_channel(64);
        let sender = Box::new(sender);
        let userdata = (&*sender as *const mpsc::SyncSender<()>).cast_mut().cast();
        // SAFETY: `frame_ready` is thread-safe; the sender outlives it.
        ok(unsafe { fy_apple_set_frame_ready(view, Some(frame_ready), userdata) });
        Self { view, ready, sender }
    }

    fn frame(&self) -> u64 {
        let mut number = 0;
        // SAFETY: a live view and a local out-value.
        ok(unsafe { fy_view_frame(self.view, &mut number) });
        number
    }

    fn wait(&self) {
        self.ready.recv_timeout(Duration::from_secs(10)).expect("a frame-ready signal within 10 s");
    }

    /// What Swift's `copyPixelBuffer` gets: the flags and the surface's
    /// pixels, or `None`.
    fn acquire(&self) -> Option<(u32, Image)> {
        let mut surface = ptr::null_mut();
        // SAFETY: a live view and a local out-value.
        let flags = unsafe { fy_apple_acquire(self.view, &mut surface) };
        if flags & FY_APPLE_SHOWN == 0 {
            assert!(surface.is_null(), "nothing written without FY_APPLE_SHOWN");
            return None;
        }
        // SAFETY: a live IOSurface, retained for us; locked while read.
        let image = unsafe {
            assert!(CFGetRetainCount(surface) >= 2, "retained for the caller as well as the ring");
            let (width, height) = (IOSurfaceGetWidth(surface), IOSurfaceGetHeight(surface));
            assert_eq!(IOSurfaceLock(surface, READ_ONLY, ptr::null_mut()), 0);
            let (base, row) = (IOSurfaceGetBaseAddress(surface), IOSurfaceGetBytesPerRow(surface));
            let mut rgba = Vec::with_capacity(width * height * 4);
            for y in 0..height {
                for p in std::slice::from_raw_parts(base.add(y * row), width * 4).chunks_exact(4) {
                    rgba.extend_from_slice(&[p[2], p[1], p[0], p[3]]);
                }
            }
            IOSurfaceUnlock(surface, READ_ONLY, ptr::null_mut());
            CFRelease(surface);
            Image { width: width as u32, height: height as u32, rgba }
        };
        Some((flags, image))
    }

    fn stats(&self) -> FyViewStats {
        let mut stats = FyViewStats::default();
        // SAFETY: a live view and a local out-value.
        ok(unsafe { fy_view_stats(self.view, &mut stats) });
        stats
    }
}

impl Drop for AppleView {
    fn drop(&mut self) {
        // SAFETY: the order the Swift plugin uses: stop the signal, then
        // free the view.
        unsafe {
            ok(fy_apple_set_frame_ready(self.view, None, ptr::null_mut()));
            fy_view_free(self.view);
        }
        let _ = &self.sender;
    }
}

#[test]
fn the_stage_1_scene_in_an_iosurface_matches_the_golden() {
    let world = World::new();
    stage1(&world);
    let view = AppleView::new(&world, 256, 256);
    assert!(view.acquire().is_none(), "nothing before the first frame");
    view.frame();
    view.wait();
    let (flags, image) = view.acquire().expect("the finished frame");
    assert_eq!(flags, FY_APPLE_SHOWN);
    assert_golden("cube", &image);
}

#[test]
fn a_resized_apple_view_shows_frames_of_the_new_size() {
    let world = World::new();
    stage1(&world);
    let view = AppleView::new(&world, 64, 64);
    view.frame();
    view.wait();
    assert_eq!(view.acquire().unwrap().1.width, 64);

    // SAFETY: a live view.
    ok(unsafe { fy_view_resize(view.view, 96, 48) });
    view.frame();
    view.wait();
    let image = view.acquire().unwrap().1;
    assert_eq!((image.width, image.height), (96, 48));
}

#[test]
fn stats_count_what_the_view_did() {
    let world = World::new();
    stage1(&world);
    let view = AppleView::new(&world, 32, 32);
    for _ in 0..20 {
        view.frame();
        view.wait();
        assert!(view.acquire().is_some());
    }
    let stats = view.stats();
    assert_eq!(stats.posted, 20);
    assert_eq!(stats.rendered + stats.coalesced + stats.skipped, 20);
    assert_eq!(stats.presented, stats.rendered, "each finished frame was taken once");
}

#[test]
fn a_headless_view_has_no_apple_side() {
    let world = World::new();
    let view = View::headless(&world, 8, 8);
    let mut surface = ptr::null_mut();
    // SAFETY: a live view and a local out-value.
    assert_eq!(unsafe { fy_apple_acquire(view.0, &mut surface) }, 0);
    assert!(surface.is_null());
    // SAFETY: a live view; no function is set.
    let status = unsafe { fy_apple_set_frame_ready(view.0, None, ptr::null_mut()) };
    assert_eq!(status, FyStatus::Unsupported);
    assert_eq!(last_error(), "not an Apple view");
    // SAFETY: null is allowed and means "no view".
    assert_eq!(unsafe { fy_apple_acquire(ptr::null(), &mut surface) }, 0);
}
