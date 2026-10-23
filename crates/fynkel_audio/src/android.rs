//! Android: cpal, once a stream is open, reads a system setting through
//! Java, with the Java VM and the app's context that `ndk-context` holds.
//! Only an app started from Rust (`android-activity`) has them there, and
//! without them cpal panics. A Flutter app's process has a VM all the same:
//! this finds it and the app's `Application`, and hands them over, once.

use std::ffi::{c_char, c_int, c_void};
use std::ptr;
use std::sync::OnceLock;

use jni_sys::{JNI_OK, JNI_VERSION_1_6, JNIEnv, JavaVM, jint, jobject, jsize};

unsafe extern "C" {
    fn dlopen(filename: *const c_char, flags: c_int) -> *mut c_void;
    fn dlsym(handle: *mut c_void, symbol: *const c_char) -> *mut c_void;
}

/// Bionic's `RTLD_DEFAULT` and `RTLD_NOW`.
const RTLD_DEFAULT: *mut c_void = ptr::null_mut();
const RTLD_NOW: c_int = 2;

static READY: OnceLock<Result<(), String>> = OnceLock::new();

/// Gives `ndk-context` the VM and the app, the first time; then says how
/// that went.
pub(crate) fn prepare() -> Result<(), String> {
    // SAFETY: run once, before cpal asks for the context.
    READY.get_or_init(|| unsafe { hand_over() }).clone()
}

type GetCreatedJavaVms = unsafe extern "system" fn(*mut *mut JavaVM, jsize, *mut jsize) -> jint;

/// # Safety
///
/// Called once, as [`prepare`] does.
unsafe fn hand_over() -> Result<(), String> {
    let name = c"JNI_GetCreatedJavaVMs".as_ptr();
    // SAFETY: looking a symbol up, by its C name; libnativehelper is where
    // the NDK puts it from Android 12, and the process has it loaded.
    let mut found = unsafe { dlsym(RTLD_DEFAULT, name) };
    if found.is_null() {
        // SAFETY: as above.
        let helper = unsafe { dlopen(c"libnativehelper.so".as_ptr(), RTLD_NOW) };
        if !helper.is_null() {
            // SAFETY: as above.
            found = unsafe { dlsym(helper, name) };
        }
    }
    if found.is_null() {
        return Err("sound on Android needs JNI_GetCreatedJavaVMs, which this Android doesn't have".to_owned());
    }
    // SAFETY: the symbol is JNI's, of this type.
    let get_vms: GetCreatedJavaVms = unsafe { std::mem::transmute(found) };
    let mut vm: *mut JavaVM = ptr::null_mut();
    let mut count: jsize = 0;
    // SAFETY: room for one VM, and its count.
    if unsafe { get_vms(&mut vm, 1, &mut count) } != JNI_OK || count < 1 || vm.is_null() {
        return Err("the app's Java VM can't be found".to_owned());
    }
    // SAFETY: a live VM, which outlives everything here.
    let invoke = unsafe { &**vm };
    let mut env: *mut JNIEnv = ptr::null_mut();
    let get_env = invoke.GetEnv.ok_or("the Java VM has no GetEnv")?;
    // SAFETY: as JNI says.
    let attached = unsafe { get_env(vm, (&raw mut env).cast(), JNI_VERSION_1_6) } == JNI_OK;
    if !attached {
        let attach = invoke.AttachCurrentThread.ok_or("the Java VM has no AttachCurrentThread")?;
        // SAFETY: as JNI says; detached again below.
        if unsafe { attach(vm, (&raw mut env).cast(), ptr::null_mut()) } != JNI_OK {
            return Err("this thread can't join the Java VM".to_owned());
        }
    }
    // SAFETY: an env of this thread's.
    let app = unsafe { application(env) };
    if !attached && let Some(detach) = invoke.DetachCurrentThread {
        // SAFETY: attached above, by this function.
        unsafe { detach(vm) };
    }
    let app = app?;
    // SAFETY: the VM and a global reference to the app, both for the
    // process's life; ndk-context is given them once.
    unsafe { ndk_context::initialize_android_context(vm.cast(), app.cast()) };
    Ok(())
}

/// A global reference to the app's `Application`, from
/// `ActivityThread.currentApplication()`.
///
/// # Safety
///
/// `env` is the current thread's.
unsafe fn application(env: *mut JNIEnv) -> Result<jobject, String> {
    // SAFETY: per this function's contract.
    let jni = unsafe { &**env };
    let missing = || "the Java VM is missing a JNI function".to_owned();
    let (find_class, get_static, call_static) = (
        jni.FindClass.ok_or_else(missing)?,
        jni.GetStaticMethodID.ok_or_else(missing)?,
        jni.CallStaticObjectMethodA.ok_or_else(missing)?,
    );
    let (new_global, delete_local) = (jni.NewGlobalRef.ok_or_else(missing)?, jni.DeleteLocalRef.ok_or_else(missing)?);
    let (check, clear) = (jni.ExceptionCheck.ok_or_else(missing)?, jni.ExceptionClear.ok_or_else(missing)?);
    // SAFETY: JNI calls on this thread's env, each checked for a Java
    // exception, which is cleared rather than left pending.
    unsafe {
        let failed = |what: &str| {
            if check(env) != 0 {
                clear(env);
            }
            Err(format!("the app's context can't be found: {what}"))
        };
        let class = find_class(env, c"android/app/ActivityThread".as_ptr());
        if class.is_null() || check(env) != 0 {
            return failed("no ActivityThread");
        }
        let method = get_static(env, class, c"currentApplication".as_ptr(), c"()Landroid/app/Application;".as_ptr());
        if method.is_null() || check(env) != 0 {
            delete_local(env, class);
            return failed("no ActivityThread.currentApplication");
        }
        let app = call_static(env, class, method, ptr::null());
        delete_local(env, class);
        if app.is_null() || check(env) != 0 {
            return failed("no application yet");
        }
        let global = new_global(env, app);
        delete_local(env, app);
        if global.is_null() {
            return failed("no room for a reference to it");
        }
        Ok(global)
    }
}
