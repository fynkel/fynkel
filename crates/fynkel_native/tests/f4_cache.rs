//! Isolated allocator gate: unchanged table queries must not encode every name.
use fynkel_contracts::{Entity, Status};
use fynkel_core::{memory, schema::EntityInfoRecord, Name, World};
use fynkel_native::{bridge, buffer, *};
use fynkel_scene::{Layer, Tags, Transform};
use fynkel_wire::WireRecord;

#[test]
fn native_table_reuses_publication_and_tracks_direct_writes() {
    unsafe {
        let mut core = World::new(Default::default());
        let commit = core
            .edit_gameplay(|edit| {
                let mut ids = Vec::new();
                for i in 0..2048 {
                    ids.push(edit.spawn(format!("object-{i}"), Transform::IDENTITY, None)?);
                }
                Ok(ids)
            })
            .unwrap();
        let e = commit.resolve(commit.value[17]).unwrap();
        let w = Box::into_raw(Box::new(FynkelWorld::with_core(core)));
        let query = |request_id| {
            let mut out = FynkelBufferView {
                data: std::ptr::null(),
                length: 0,
            };
            assert_eq!(
                fynkel_world_query(
                    w,
                    &FynkelQueryDesc {
                        kind: bridge::QUERY_ENTITIES,
                        request_id,
                        entity: Entity::NULL
                    },
                    &mut out
                ),
                Status::Ok
            );
            out
        };
        query(0);
        let allocations = memory::allocation_count();
        for id in 1..101 {
            let view = query(id);
            let bytes = std::slice::from_raw_parts(view.data, view.length as usize);
            assert_eq!(u32::from_le_bytes(bytes[20..24].try_into().unwrap()), id);
        }
        assert_eq!(
            memory::allocation_count(),
            allocations,
            "unchanged publication must not allocate names/rows"
        );
        (*w).core().ecs().get_mut::<Name>(e).unwrap().value = "renamed".into();
        (*w).core_mut()
            .ecs_mut()
            .insert(e, Layer { layer: 3 })
            .unwrap();
        (*w).core_mut()
            .ecs_mut()
            .insert(e, Tags { bits: 8 })
            .unwrap();
        let view = query(101);
        let (_, records, tail) = buffer::validate(
            std::slice::from_raw_parts(view.data, view.length as usize),
            buffer::BufferKind::Query,
        )
        .unwrap();
        let row = records
            .chunks_exact(EntityInfoRecord::SIZE)
            .map(|b| EntityInfoRecord::decode(b, tail).unwrap())
            .find(|r| r.entity == e)
            .unwrap();
        assert_eq!((row.name.as_str(), row.layer, row.tags), ("renamed", 3, 8));
        fynkel_world_destroy(w);
    }
}
