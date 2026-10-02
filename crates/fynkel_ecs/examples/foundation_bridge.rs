//! Isolated F0 candidate kernel. Not the shipping runtime/extension ABI.
//! Native owns Health; Dart edits a reusable snapshot and commits once per batch.
use fynkel_contracts::Entity;
use fynkel_ecs::{internal::Handle, EntityBuilder, World};
use std::panic::{catch_unwind, AssertUnwindSafe};

#[derive(Clone, Copy)]
struct Health {
    hp: f64,
    poison: f64,
}

#[repr(C)]
struct Row {
    entity: Entity,
    hp: f64,
    poison: f64,
}

struct Kernel {
    world: World,
    snapshot: Vec<Row>,
    token: u64,
    open: bool,
    poisoned: bool,
}

fn guarded<T>(kernel: &mut Kernel, fallback: T, run: impl FnOnce(&mut Kernel) -> T) -> T {
    if kernel.poisoned {
        return fallback;
    }
    match catch_unwind(AssertUnwindSafe(|| run(kernel))) {
        Ok(value) => value,
        Err(_) => {
            kernel.poisoned = true;
            fallback
        }
    }
}

#[no_mangle]
pub extern "C" fn foundation_create(count: u32) -> *mut std::ffi::c_void {
    catch_unwind(|| {
        // Candidate is isolated. Its world namespace allocator is NOT a solution
        // to F1.1 and these handles must never be exchanged with shipping worlds.
        let mut world = World::new();
        for _ in 0..count {
            world.spawn(EntityBuilder::new().with(Health {
                hp: 100.0,
                poison: 2.0,
            }));
        }
        Box::into_raw(Box::new(Kernel {
            world,
            snapshot: Vec::with_capacity(count as usize),
            token: 0,
            open: false,
            poisoned: false,
        })) as *mut std::ffi::c_void
    })
    .unwrap_or(std::ptr::null_mut())
}

/// Caller must supply a live kernel and writable token; view expires at next call.
///
/// # Safety
/// `pointer` must be a live, exclusively accessed allocation from foundation_create;
/// `token` must point to writable u64 storage. Do not retain views across calls.
#[no_mangle]
pub unsafe extern "C" fn foundation_snapshot(
    pointer: *mut std::ffi::c_void,
    token: *mut u64,
) -> *mut std::ffi::c_void {
    if pointer.is_null() || token.is_null() {
        return std::ptr::null_mut();
    }
    guarded(
        &mut *(pointer as *mut Kernel),
        std::ptr::null_mut(),
        |kernel| {
            kernel.snapshot.clear();
            for (handle, health) in kernel.world.query::<(&Handle, &Health)>().iter() {
                kernel.snapshot.push(Row {
                    entity: handle.0,
                    hp: health.hp,
                    poison: health.poison,
                });
            }
            kernel.token = kernel
                .token
                .checked_add(1)
                .expect("snapshot token exhausted");
            kernel.open = true;
            *token = kernel.token;
            kernel.snapshot.as_mut_ptr() as *mut std::ffi::c_void
        },
    )
}

/// Validate the entire edit before touching authority. Consume snapshot even on rejection.
///
/// # Safety
/// `pointer` must be a live, exclusively accessed allocation from foundation_create.
/// Only edit the two value columns of the current snapshot within its allocated length.
#[no_mangle]
pub unsafe extern "C" fn foundation_commit(pointer: *mut std::ffi::c_void, token: u64) -> i32 {
    if pointer.is_null() {
        return -1;
    }
    guarded(&mut *(pointer as *mut Kernel), -1, |kernel| {
        if !kernel.open || kernel.token != token {
            return 2;
        }
        kernel.open = false;
        // Identity is immutable snapshot metadata, not editable authored data.
        let valid = kernel
            .snapshot
            .iter()
            .zip(kernel.world.query::<(&Handle, &Health)>().iter())
            .all(|(row, (handle, _))| {
                row.entity == handle.0
                    && row.hp.is_finite()
                    && row.hp >= 0.0
                    && row.poison.is_finite()
                    && row.poison >= 0.0
            });
        if !valid {
            return 1;
        }
        for row in &kernel.snapshot {
            let mut health = kernel.world.get_mut::<Health>(row.entity).unwrap();
            health.hp = row.hp;
            health.poison = row.poison;
        }
        0
    })
}

#[no_mangle]
/// # Safety
/// Pointer is null or a live allocation from foundation_create. Dispose once,
/// after all calls and views have ended; no concurrent access is allowed.
pub unsafe extern "C" fn foundation_dispose(pointer: *mut std::ffi::c_void) {
    if !pointer.is_null() {
        drop(Box::from_raw(pointer as *mut Kernel));
    }
}
