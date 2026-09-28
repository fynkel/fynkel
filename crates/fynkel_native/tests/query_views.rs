//! Foundation 4.9: every query kind answers into its own buffer, so a
//! view a host still holds survives a query of another kind (the SDK
//! keeps one cached view per kind and reads them in place). All kinds
//! shared one buffer: a `components` query rewrote the `entities` view
//! the host was still walking — or freed it when the buffer grew.

use std::ptr;

use fynkel_contracts::Status;

use fynkel_native::bridge::{QUERY_ALL_ENTITIES, QUERY_SYSTEMS};
use fynkel_native::buffer::{self, BufferKind};
use fynkel_native::*;

fn bytes(v: &FynkelBufferView) -> &'static [u8] {
    if v.length == 0 {
        return &[];
    }
    unsafe { std::slice::from_raw_parts(v.data, v.length as usize) }
}

unsafe fn query(w: *mut FynkelWorld, kind: u32, request_id: u32) -> FynkelBufferView {
    let desc = FynkelQueryDesc {
        kind,
        request_id,
        entity: 0,
    };
    let mut v = FynkelBufferView {
        data: ptr::null(),
        length: 0,
    };
    assert_eq!(fynkel_world_query(w, &desc, &mut v), Status::Ok);
    v
}

#[test]
fn a_held_view_survives_a_query_of_another_kind() {
    let rt = fynkel_runtime_create();
    let w = unsafe { fynkel_world_create(rt, ptr::null()) };
    unsafe {
        let entities = query(w, QUERY_ALL_ENTITIES, 1);
        let kept = bytes(&entities).to_vec();

        query(w, QUERY_SYSTEMS, 2);
        assert_eq!(
            bytes(&entities),
            &kept[..],
            "a systems query rewrote the entities view"
        );

        let mut v = FynkelBufferView {
            data: ptr::null(),
            length: 0,
        };
        let none = buffer::empty(BufferKind::PhysicsQueries);
        assert_eq!(
            fynkel_world_physics_query(w, none.as_ptr(), none.len() as u32, &mut v),
            Status::Ok
        );
        assert_eq!(
            bytes(&entities),
            &kept[..],
            "a physics query rewrote the entities view"
        );

        let mut info = FynkelProjectionInfo::default();
        let anchors = buffer::empty(BufferKind::ProjectionAnchors);
        assert_eq!(
            fynkel_world_project(
                w,
                64,
                64,
                anchors.as_ptr(),
                anchors.len() as u32,
                &mut info,
                &mut v
            ),
            Status::Ok
        );
        assert_eq!(
            bytes(&entities),
            &kept[..],
            "a projection rewrote the entities view"
        );

        // The same kind again replaces it: the view's documented lifetime.
        let again = query(w, QUERY_ALL_ENTITIES, 3);
        assert_eq!(again.data, entities.data, "a kind reuses its own buffer");

        fynkel_world_destroy(w);
        fynkel_runtime_destroy(rt);
    }
}
