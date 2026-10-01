//! Foundation 4.5: the ABI entries never hold two `&mut` to one world.
//! A plain run passes either way; the check is Miri's (Stacked Borrows):
//!
//! ```sh
//! cargo +nightly miri test -p fynkel_native --test aliasing
//! ```

use std::ptr;

use fynkel_contracts::Status;
use fynkel_native::*;

#[test]
fn reading_input_and_events_borrows_the_world_once() {
    let rt = fynkel_runtime_create();
    let w = unsafe { fynkel_world_create(rt, ptr::null(), std::ptr::null_mut()) };
    assert!(!w.is_null());
    unsafe {
        let view = fynkel_world_input_view(w);
        assert!(!view.data.is_null());
        let events = fynkel_world_drain_events(w);
        assert!(!events.data.is_null());
        // The per-frame calls around them, once each.
        let mut plan = FynkelFramePlan::default();
        assert_eq!(
            fynkel_world_begin_frame(w, 1.0 / 60.0, 1.0, &mut plan),
            Status::Ok
        );
        for _ in 0..plan.steps {
            let mut r = FynkelStepResult::default();
            assert_eq!(fynkel_world_step(w, &mut r), Status::Ok);
        }
        let _ = fynkel_world_snapshot(w);
        let _ = fynkel_world_diagnostics(w);
        let _ = fynkel_world_drain_events(w);
        let _ = fynkel_world_input_view(w);
        fynkel_world_destroy(w);
        fynkel_runtime_destroy(rt);
    }
}
