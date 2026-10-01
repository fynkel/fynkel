//! Manual dynamic-loader qualification. Build the native cdylib first, then:
//! cargo test --release -p fynkel_native --test identity_libraries -- --ignored
//! Copies give the loader genuinely independent engine images; a host retains
//! the authority independently of every module handle.
use std::{path::Path, ptr};

use fynkel_contracts::{Entity, Status};
use fynkel_native::*;
use fynkel_scene::Transform;
use libloading::Library;
#[path = "support/edit.rs"]
mod edit;
use edit::{Api, Begin, Edit, End, Exchange};

type CreateRuntime = unsafe extern "C" fn() -> *mut FynkelRuntime;
type CreateWorld = unsafe extern "C" fn(
    *mut FynkelRuntime,
    *const FynkelWorldDesc,
    *mut Status,
) -> *mut FynkelWorld;
type Step = unsafe extern "C" fn(*mut FynkelWorld, *mut FynkelStepResult) -> Status;

struct Module(Library);
impl Module {
    unsafe fn open(path: &Path) -> Self {
        let library = Library::new(path).unwrap();
        for retired in [
            b"fynkel_world_submit_commands\0".as_slice(),
            b"fynkel_world_gameplay_adopt\0".as_slice(),
        ] {
            assert!(
                library.get::<unsafe extern "C" fn()>(retired).is_err(),
                "retired compatibility symbol remains exported"
            );
        }
        Self(library)
    }
    unsafe fn authority(&self) -> *const FynkelIdentityAuthority {
        self.0
            .get::<unsafe extern "C" fn() -> *const FynkelIdentityAuthority>(
                b"fynkel_identity_authority\0",
            )
            .unwrap()()
    }
    unsafe fn attach(&self, authority: *const FynkelIdentityAuthority) -> Status {
        self.0
            .get::<unsafe extern "C" fn(*const FynkelIdentityAuthority) -> Status>(
                b"fynkel_identity_attach\0",
            )
            .unwrap()(authority)
    }
    unsafe fn contains(&self, world: *mut FynkelWorld, entity: Entity) -> bool {
        self.0
            .get::<unsafe extern "C" fn(*const FynkelWorld, Entity) -> i32>(
                b"fynkel_world_contains\0",
            )
            .unwrap()(world, entity)
            == 1
    }
    unsafe fn edit(&self, world: *mut FynkelWorld) -> Edit {
        Edit::with_api(
            world,
            Api {
                begin: *self
                    .0
                    .get::<Begin>(b"fynkel_world_gameplay_begin\0")
                    .unwrap(),
                exchange: *self
                    .0
                    .get::<Exchange>(b"fynkel_world_gameplay_exchange\0")
                    .unwrap(),
                end: *self.0.get::<End>(b"fynkel_world_gameplay_end\0").unwrap(),
            },
        )
    }
    unsafe fn step(&self, world: *mut FynkelWorld) -> Status {
        self.0.get::<Step>(b"fynkel_world_step\0").unwrap()(world, &mut FynkelStepResult::default())
    }
    unsafe fn spawn(&self) -> (*mut FynkelRuntime, *mut FynkelWorld, Entity) {
        let rt = self
            .0
            .get::<CreateRuntime>(b"fynkel_runtime_create\0")
            .unwrap()();
        assert!(!rt.is_null());
        let mut status = Status::InvalidArgument;
        let world = self.0.get::<CreateWorld>(b"fynkel_world_create\0").unwrap()(
            rt,
            ptr::null(),
            &mut status,
        );
        assert_eq!(status, Status::Ok);
        assert!(!world.is_null());
        let mut edit = self.edit(world);
        let pending = edit.spawn("probe", Transform::IDENTITY, None);
        let entity = edit.commit().resolve(pending);
        (rt, world, entity)
    }
    unsafe fn destroy(&self, rt: *mut FynkelRuntime, world: *mut FynkelWorld) {
        self.0
            .get::<unsafe extern "C" fn(*mut FynkelWorld)>(b"fynkel_world_destroy\0")
            .unwrap()(world);
        self.0
            .get::<unsafe extern "C" fn(*mut FynkelRuntime)>(b"fynkel_runtime_destroy\0")
            .unwrap()(rt);
    }
    fn close(self) {
        self.0.close().unwrap();
    }
}

#[test]
#[ignore = "requires a freshly built native cdylib; run the authority gate"]
fn shared_host_authority_survives_independent_modules_and_replacement() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let source = std::env::var_os("FYNKEL_IDENTITY_TEST_LIBRARY")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            root.join("target/release").join(format!(
                "{}fynkel_native{}",
                std::env::consts::DLL_PREFIX,
                std::env::consts::DLL_SUFFIX
            ))
        });
    assert!(
        source.exists(),
        "build native cdylib first: {}",
        source.display()
    );
    let dir = std::env::temp_dir().join(format!("fynkel-identity-modules-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let copy = |index| {
        let path = dir.join(format!("engine-{index}{}", std::env::consts::DLL_SUFFIX));
        std::fs::copy(&source, &path).unwrap();
        path
    };
    unsafe {
        let first_path = copy(0);
        let first = Module::open(&first_path);
        let authority = first.authority();
        assert!(!authority.is_null());
        assert_eq!(first.attach(ptr::null()), Status::InvalidArgument);
        assert_eq!(first.attach(authority), Status::Ok);
        let (rt1, w1, e1) = first.spawn();
        let second_path = copy(1);
        let second = Module::open(&second_path);
        assert_eq!(second.attach(authority), Status::Ok);
        assert_eq!(second.authority(), authority);
        let (rt2, w2, e2) = second.spawn();
        assert_ne!(e1.world(), e2.world());
        assert_eq!(e1.slot(), e2.slot());
        assert_eq!(e1.generation(), e2.generation());
        assert!(!second.contains(w2, e1));
        assert!(second.contains(w2, e2));
        let mut edit = second.edit(w2);
        assert_eq!(edit.try_destroy(e1), Status::CommandsRejected);
        drop(edit);
        assert_eq!(second.step(w2), Status::Ok);
        assert!(second.contains(w2, e2));
        // A truly independent image bootstraps another address and cannot
        // switch authorities after startup. This also verifies the copies
        // were not folded into one module by the loader.
        let isolated_path = copy(2);
        let isolated = Module::open(&isolated_path);
        let isolated_authority = isolated.authority();
        assert_ne!(isolated_authority, authority);
        assert_eq!(isolated.attach(authority), Status::InvalidArgument);
        assert_eq!(second.attach(isolated_authority), Status::InvalidArgument);
        isolated.close();
        first.destroy(rt1, w1);
        second.destroy(rt2, w2);
        first.close();
        second.close();
        // Neither originating module handle survives. New images must use
        // the host-retained storage, without a function pointer into old code.
        let mut last = e2.world();
        for i in 3..13 {
            let path = copy(i);
            let replacement = Module::open(&path);
            assert_eq!(replacement.attach(authority), Status::Ok);
            let (rt, w, entity) = replacement.spawn();
            assert!(entity.world() > last);
            assert!(!replacement.contains(w, e1));
            assert!(!replacement.contains(w, e2));
            last = entity.world();
            replacement.destroy(rt, w);
            replacement.close();
        }
        eprintln!("identity_modules: 2 shared engines, incompatible authority rejected, 10 unloaded/replaced engines; final world {}", last.0);
    }
    std::fs::remove_dir_all(dir).unwrap();
}
