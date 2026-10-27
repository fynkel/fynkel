//! Android: cpal, once a stream is open, reads a system setting through
//! Java, with the Java VM and the app's context that `ndk-context` holds.
//! Only an app started from Rust (`android-activity`) has them there, and
//! without them cpal panics. A Flutter app's process has a VM all the same:
//! this finds it and the app's `Application`, and hands them over, once.
//!
//! The same VM and app give the sound its audio focus (docs/stage-11.md
//! §3.4): asked for with `Session::Playback`, so the player's music stops,
//! as on iOS; and whether a call is on (or ringing), so the game's sound
//! waits for it to end. Through JNI, so a game needs no Java of its own.

use std::ffi::{c_char, c_int, c_void};
use std::ptr;
use std::sync::OnceLock;

use jni_sys::{JNI_OK, JNI_VERSION_1_6, JNIEnv, JavaVM, jint, jmethodID, jobject, jsize, jvalue};

unsafe extern "C" {
    fn dlopen(filename: *const c_char, flags: c_int) -> *mut c_void;
    fn dlsym(handle: *mut c_void, symbol: *const c_char) -> *mut c_void;
}

/// Bionic's `RTLD_DEFAULT` and `RTLD_NOW`.
const RTLD_DEFAULT: *mut c_void = ptr::null_mut();
const RTLD_NOW: c_int = 2;

static READY: OnceLock<Result<Java, String>> = OnceLock::new();
static MANAGER: OnceLock<Result<Manager, String>> = OnceLock::new();

/// The VM and the app, for the process's life (as addresses, to be shared).
#[derive(Clone, Copy, Debug)]
struct Java {
    vm: usize,
    app: usize,
}

/// The app's `AudioManager`, a global reference, and the methods used.
#[derive(Debug)]
struct Manager {
    object: usize,
    get_mode: usize,
    request: usize,
    abandon: usize,
}

/// `AudioManager`'s constants.
const STREAM_MUSIC: jint = 3;
const AUDIOFOCUS_GAIN: jint = 1;

/// Gives `ndk-context` the VM and the app, the first time; then says how
/// that went.
pub(crate) fn prepare() -> Result<(), String> {
    // SAFETY: run once, before cpal asks for the context.
    READY.get_or_init(|| unsafe { hand_over() }).clone().map(|_| ())
}

/// Asks for the audio focus, for good, when `wanted`, so other apps'
/// sound stops; gives it up when not. Fails only when it can't be asked.
pub(crate) fn focus(wanted: bool) -> Result<(), String> {
    let manager = manager()?;
    // SAFETY: a live env, the manager's methods on it, and a null
    // listener, which the deprecated forms take: the focus is the
    // manager's, given up with the same null.
    with_env(|env| unsafe {
        let jni = &**env;
        if wanted {
            let call = jni.CallIntMethodA.ok_or("no CallIntMethodA")?;
            let args = [jvalue { l: ptr::null_mut() }, jvalue { i: STREAM_MUSIC }, jvalue { i: AUDIOFOCUS_GAIN }];
            call(env, manager.object as jobject, manager.request as jmethodID, args.as_ptr());
        } else {
            let call = jni.CallIntMethodA.ok_or("no CallIntMethodA")?;
            let args = [jvalue { l: ptr::null_mut() }];
            call(env, manager.object as jobject, manager.abandon as jmethodID, args.as_ptr());
        }
        thrown(env, "the audio focus")
    })
}

/// The phone's mode (`AudioManager.getMode`): a call ringing, on, and so
/// on; `None` when it can't be asked.
pub(crate) fn mode() -> Option<i32> {
    let manager = manager().ok()?;
    // SAFETY: a live env, and the manager's getMode on it.
    with_env(|env| unsafe {
        let call = (**env).CallIntMethodA.ok_or("no CallIntMethodA")?;
        let mode = call(env, manager.object as jobject, manager.get_mode as jmethodID, ptr::null());
        thrown(env, "the phone's mode")?;
        Ok(mode)
    })
    .ok()
}

fn manager() -> Result<&'static Manager, String> {
    let java = READY.get_or_init(|| unsafe { hand_over() }).clone()?;
    // SAFETY: a live env; the app is a global reference.
    MANAGER
        .get_or_init(|| with_env(|env| unsafe { find_manager(env, java.app as jobject) }))
        .as_ref()
        .map_err(Clone::clone)
}

/// The app's `getSystemService("audio")`, and its methods.
///
/// # Safety
///
/// `env` is the current thread's; `app` a live `Context`.
unsafe fn find_manager(env: *mut JNIEnv, app: jobject) -> Result<Manager, String> {
    // SAFETY: per this function's contract; each call checked for a Java
    // exception, and local references deleted.
    unsafe {
        let jni = &**env;
        let missing = || "the Java VM is missing a JNI function".to_owned();
        let get_class = jni.GetObjectClass.ok_or_else(missing)?;
        let find_class = jni.FindClass.ok_or_else(missing)?;
        let method = jni.GetMethodID.ok_or_else(missing)?;
        let call = jni.CallObjectMethodA.ok_or_else(missing)?;
        let new_string = jni.NewStringUTF.ok_or_else(missing)?;
        let (new_global, delete_local) =
            (jni.NewGlobalRef.ok_or_else(missing)?, jni.DeleteLocalRef.ok_or_else(missing)?);
        let context = get_class(env, app);
        let service =
            method(env, context, c"getSystemService".as_ptr(), c"(Ljava/lang/String;)Ljava/lang/Object;".as_ptr());
        delete_local(env, context);
        thrown(env, "Context.getSystemService")?;
        let name = new_string(env, c"audio".as_ptr());
        let args = [jvalue { l: name }];
        let object = call(env, app, service, args.as_ptr());
        delete_local(env, name);
        thrown(env, "the audio service")?;
        if object.is_null() {
            return Err("the app has no audio service".to_owned());
        }
        let class = find_class(env, c"android/media/AudioManager".as_ptr());
        thrown(env, "AudioManager")?;
        let listener = c"(Landroid/media/AudioManager$OnAudioFocusChangeListener;II)I";
        let get_mode = method(env, class, c"getMode".as_ptr(), c"()I".as_ptr());
        let request = method(env, class, c"requestAudioFocus".as_ptr(), listener.as_ptr());
        let abandon = method(
            env,
            class,
            c"abandonAudioFocus".as_ptr(),
            c"(Landroid/media/AudioManager$OnAudioFocusChangeListener;)I".as_ptr(),
        );
        delete_local(env, class);
        thrown(env, "AudioManager's methods")?;
        let global = new_global(env, object);
        delete_local(env, object);
        if global.is_null() || get_mode.is_null() || request.is_null() || abandon.is_null() {
            return Err("AudioManager isn't as expected".to_owned());
        }
        Ok(Manager {
            object: global as usize,
            get_mode: get_mode as usize,
            request: request as usize,
            abandon: abandon as usize,
        })
    }
}

/// An error, the exception cleared, if the last JNI call threw.
///
/// # Safety
///
/// `env` is the current thread's.
unsafe fn thrown(env: *mut JNIEnv, what: &str) -> Result<(), String> {
    // SAFETY: per this function's contract.
    unsafe {
        let jni = &**env;
        let (Some(check), Some(clear)) = (jni.ExceptionCheck, jni.ExceptionClear) else { return Ok(()) };
        if check(env) != 0 {
            clear(env);
            return Err(format!("{what} threw"));
        }
    }
    Ok(())
}

/// Runs `f` with this thread's env, joining it to the VM first if it
/// isn't (for good: the game's thread asks again each half second).
fn with_env<T>(f: impl FnOnce(*mut JNIEnv) -> Result<T, String>) -> Result<T, String> {
    let java = READY.get().ok_or("no Java VM yet")?.clone()?;
    let vm = java.vm as *mut JavaVM;
    // SAFETY: a live VM, for the process's life.
    let invoke = unsafe { &**vm };
    let mut env: *mut JNIEnv = ptr::null_mut();
    let get_env = invoke.GetEnv.ok_or("the Java VM has no GetEnv")?;
    // SAFETY: as JNI says.
    if unsafe { get_env(vm, (&raw mut env).cast(), JNI_VERSION_1_6) } != JNI_OK {
        let attach = invoke.AttachCurrentThreadAsDaemon.ok_or("the Java VM has no AttachCurrentThreadAsDaemon")?;
        // SAFETY: as JNI says; a daemon thread doesn't keep the VM up.
        if unsafe { attach(vm, (&raw mut env).cast(), ptr::null_mut()) } != JNI_OK {
            return Err("this thread can't join the Java VM".to_owned());
        }
    }
    f(env)
}

type GetCreatedJavaVms = unsafe extern "system" fn(*mut *mut JavaVM, jsize, *mut jsize) -> jint;

/// # Safety
///
/// Called once, as [`prepare`] does.
unsafe fn hand_over() -> Result<Java, String> {
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
    Ok(Java { vm: vm as usize, app: app as usize })
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
