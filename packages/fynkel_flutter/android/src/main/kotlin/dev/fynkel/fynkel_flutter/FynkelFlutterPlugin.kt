package dev.fynkel.fynkel_flutter

import android.app.Activity
import android.content.ComponentCallbacks2
import android.content.Context
import android.content.res.Configuration
import android.util.Log
import android.view.SurfaceHolder
import android.view.SurfaceView
import android.view.ViewGroup
import android.widget.FrameLayout
import io.flutter.embedding.android.FlutterView
import io.flutter.embedding.engine.plugins.FlutterPlugin
import io.flutter.embedding.engine.plugins.activity.ActivityAware
import io.flutter.embedding.engine.plugins.activity.ActivityPluginBinding
import io.flutter.plugin.common.MethodCall
import io.flutter.plugin.common.MethodChannel
import io.flutter.view.TextureRegistry

/**
 * The method channel `fynkel_flutter` on Android. Dart owns each engine view (made through
 * dart:ffi) and passes its address here; this class gives the view the `Surface` of a Flutter
 * `SurfaceProducer` to draw into, which Flutter shows as a texture.
 *
 * A producer makes a new surface at every resize. So the size and the surface always reach the
 * engine together, from here: Dart sends `resize`, never `fy_view_resize` (docs/stage-1.md §3.5).
 *
 * The producer keeps its surface while the app is in the background (`manual`). With
 * `resetInBackground`, Flutter 3.47's Impeller GLES texture keeps the last 6 images of every
 * surface it takes away: 58 MB a trip on a Galaxy Note10+ (§3.5).
 *
 * In a [FynkelActivity], whose Flutter view is see-through, a view asking to be on screen draws to
 * a `SurfaceView` of its own instead, behind Flutter's view where the `FynkelView` is laid out, and
 * Android shows its frames without Flutter (docs/android-surface.md). Dart moves it as the widget
 * moves (`place`), and puts it over the other views when it lies within one (`over`).
 *
 * A hot restart starts a new Dart isolate without disposing anything. The first `FynkelView` of
 * each isolate calls `reset` before it registers, and every view still here then belongs to an
 * isolate that is gone: this class releases and frees those itself.
 */
class FynkelFlutterPlugin : FlutterPlugin, MethodChannel.MethodCallHandler, ActivityAware {
    private class Entry(val view: Long, val producer: TextureRegistry.SurfaceProducer)

    private lateinit var channel: MethodChannel
    private lateinit var textures: TextureRegistry
    private val live = HashMap<Long, Entry>()

    /** Views on screen, by an id of their own, apart from the textures'. */
    private val onScreen = HashMap<Long, OnScreen>()
    private var nextOnScreen = 1L shl 40
    private var activity: Activity? = null
    private var toldNotOnScreen = false
    private var context: Context? = null

    /**
     * Android's memory warnings, passed to Dart as `lowMemory`. Flutter's own warning comes for any
     * level from `TRIM_MEMORY_RUNNING_LOW` up, and so for `TRIM_MEMORY_UI_HIDDEN`, which Android
     * sends each time the app goes away (Home), short of memory or not: a game told to lower its
     * quality would lower it on every trip. Dart's views hear these instead on Android.
     */
    private val memory = object : ComponentCallbacks2 {
        override fun onTrimMemory(level: Int) {
            if (warns(level)) channel.invokeMethod("lowMemory", mapOf("level" to level))
        }

        @Deprecated("Android's own; onTrimMemory is what's heard")
        override fun onLowMemory() {
            channel.invokeMethod("lowMemory", mapOf("level" to ComponentCallbacks2.TRIM_MEMORY_COMPLETE))
        }

        override fun onConfigurationChanged(newConfig: Configuration) {}
    }

    /**
     * A view drawn straight to the screen: a `SurfaceView` behind Flutter's view, in physical pixels
     * from its top left. It has one while there is an activity to put it in; Android makes and takes
     * away its surface (the app going to the background and back), and the view follows.
     */
    private inner class OnScreen(val id: Long, val view: Long) : SurfaceHolder.Callback {
        var left = 0
        var top = 0
        var frameWidth = 1
        var frameHeight = 1
        var width = 1
        var height = 1
        var over = false
        var surface: SurfaceView? = null

        fun params() = FrameLayout.LayoutParams(frameWidth, frameHeight).also {
            it.leftMargin = left
            it.topMargin = top
        }

        /** Makes its `SurfaceView` in [activity], behind Flutter's view. */
        fun show(activity: Activity) {
            hide()
            val content = activity.findViewById<FrameLayout>(android.R.id.content) ?: return
            val surface = SurfaceView(activity)
            // Only before it's in a window.
            surface.setZOrderMediaOverlay(over)
            surface.holder.setFixedSize(width, height)
            surface.holder.addCallback(this)
            surface.layoutParams = params()
            val flutter = (0 until content.childCount).firstOrNull { content.getChildAt(it) is FlutterView }
            content.addView(surface, flutter ?: 0)
            this.surface = surface
        }

        /** Lets go of its `SurfaceView`: the engine view first. */
        fun hide() {
            val surface = surface ?: return
            this.surface = null
            surface.holder.removeCallback(this)
            FynkelNative.detach(view)
            (surface.parent as? ViewGroup)?.removeView(surface)
        }

        override fun surfaceCreated(holder: SurfaceHolder) {}

        override fun surfaceChanged(holder: SurfaceHolder, format: Int, w: Int, h: Int) {
            // The old surface first: one window takes one producer at a time.
            FynkelNative.detach(view)
            // Tried twice, as a texture's: a surface can fail once while it's being made.
            val once = FynkelNative.attach(view, holder.surface, w, h, true) ?: return
            Log.w(TAG, "couldn't draw to the screen, trying again: $once")
            val error = FynkelNative.attach(view, holder.surface, w, h, true) ?: return
            Log.e(TAG, "couldn't draw to the screen: $error")
            channel.invokeMethod("failed", mapOf("texture" to id, "message" to "couldn't draw to the screen: $error"))
        }

        override fun surfaceDestroyed(holder: SurfaceHolder) {
            // The surface may be released as soon as this returns.
            FynkelNative.detach(view)
        }
    }

    override fun onAttachedToActivity(binding: ActivityPluginBinding) {
        activity = binding.activity
        for (entry in onScreen.values) entry.show(binding.activity)
    }

    override fun onDetachedFromActivityForConfigChanges() = onDetachedFromActivity()

    override fun onReattachedToActivityForConfigChanges(binding: ActivityPluginBinding) = onAttachedToActivity(binding)

    override fun onDetachedFromActivity() {
        for (entry in onScreen.values) entry.hide()
        activity = null
    }

    /** `register` for a view that asked to be on screen, in an activity that lets it. */
    private fun registerOnScreen(view: Long, width: Int, height: Int, call: MethodCall, activity: Activity): Long {
        val entry = OnScreen(nextOnScreen++, view)
        entry.width = width
        entry.height = height
        entry.left = call.argument<Int>("left") ?: 0
        entry.top = call.argument<Int>("top") ?: 0
        entry.frameWidth = (call.argument<Int>("frameWidth") ?: width).coerceAtLeast(1)
        entry.frameHeight = (call.argument<Int>("frameHeight") ?: height).coerceAtLeast(1)
        onScreen[entry.id] = entry
        entry.show(activity)
        return entry.id
    }

    /** `place`, `over`, `resize` and `unregister` for a view on screen. */
    private fun onScreenCall(entry: OnScreen, call: MethodCall, result: MethodChannel.Result) {
        when (call.method) {
            "place" -> {
                entry.left = call.argument<Int>("left") ?: entry.left
                entry.top = call.argument<Int>("top") ?: entry.top
                entry.frameWidth = (call.argument<Int>("width") ?: entry.frameWidth).coerceAtLeast(1)
                entry.frameHeight = (call.argument<Int>("height") ?: entry.frameHeight).coerceAtLeast(1)
                entry.surface?.layoutParams = entry.params()
            }

            "over" -> {
                val over = call.argument<Boolean>("over") ?: false
                if (over != entry.over) {
                    entry.over = over
                    // A SurfaceView's layer is set before it's in a window: a new one.
                    activity?.let { if (entry.surface != null) entry.show(it) }
                }
            }

            "resize" -> {
                val width = call.argument<Int>("width")
                val height = call.argument<Int>("height")
                if (width == null || height == null) return result.error("arguments", "resize needs a width and height", null)
                entry.width = width
                entry.height = height
                // A new size comes back through surfaceChanged.
                entry.surface?.holder?.setFixedSize(width, height)
            }

            "unregister" -> {
                onScreen.remove(entry.id)
                // After this Dart frees the view.
                entry.hide()
            }

            else -> return result.notImplemented()
        }
        result.success(null)
    }

    override fun onAttachedToEngine(binding: FlutterPlugin.FlutterPluginBinding) {
        // Compiled pipelines are kept here, so a later run's first frame doesn't compile them.
        FynkelNative.cacheDir(binding.applicationContext.cacheDir.absolutePath)
        textures = binding.textureRegistry
        channel = MethodChannel(binding.binaryMessenger, "fynkel_flutter")
        channel.setMethodCallHandler(this)
        context = binding.applicationContext.also { it.registerComponentCallbacks(memory) }
    }

    override fun onDetachedFromEngine(binding: FlutterPlugin.FlutterPluginBinding) {
        context?.unregisterComponentCallbacks(memory)
        context = null
        channel.setMethodCallHandler(null)
        releaseOrphans()
    }

    override fun onMethodCall(call: MethodCall, result: MethodChannel.Result) {
        call.argument<Number>("texture")?.toLong()?.let { onScreen[it] }?.let { return onScreenCall(it, call, result) }
        when (call.method) {
            "register" -> {
                val view = call.argument<Number>("view")?.toLong()
                val width = call.argument<Int>("width")
                val height = call.argument<Int>("height")
                if (view == null || view == 0L || width == null || height == null) {
                    result.error("arguments", "register needs view, width and height", null)
                    return
                }
                if (call.argument<Boolean>("onScreen") == true) {
                    val activity = activity
                    if (activity is FynkelActivity) {
                        result.success(mapOf("id" to registerOnScreen(view, width, height, call, activity), "onScreen" to true))
                        return
                    }
                    if (!toldNotOnScreen) {
                        toldNotOnScreen = true
                        Log.i(TAG, "the activity isn't a FynkelActivity, so views are textures Flutter draws (slower on some phones)")
                    }
                }
                val producer = textures.createSurfaceProducer(TextureRegistry.SurfaceLifecycle.manual)
                val entry = Entry(view, producer)
                // Flutter 3.47 never takes a `manual` producer's surface away by itself. Should a
                // later one do so, the view lets go of it first and takes the new one.
                producer.setCallback(object : TextureRegistry.SurfaceProducer.Callback {
                    // A new surface, after one was taken away.
                    // Dart is told when the view can't draw to it, so a new view takes its place.
                    override fun onSurfaceAvailable() {
                        Log.i(TAG, "texture ${producer.id()}: a new surface")
                        val error = attach(entry) ?: return
                        Log.e(TAG, "couldn't draw to the new surface: $error")
                        channel.invokeMethod(
                            "failed",
                            mapOf("texture" to producer.id(), "message" to "couldn't draw to the new surface: $error"),
                        )
                    }

                    // The surface is about to be released.
                    override fun onSurfaceCleanup() {
                        Log.i(TAG, "texture ${producer.id()}: the surface goes")
                        FynkelNative.detach(view)
                    }
                })
                producer.setSize(width, height)
                val error = attach(entry)
                if (error != null) {
                    release(entry)
                    result.error("attach", error, null)
                    return
                }
                live[producer.id()] = entry
                result.success(producer.id())
            }

            "resize" -> {
                val entry = call.argument<Number>("texture")?.toLong()?.let { live[it] }
                val width = call.argument<Int>("width")
                val height = call.argument<Int>("height")
                if (entry == null || width == null || height == null) {
                    result.error("arguments", "resize needs a registered texture, width and height", null)
                    return
                }
                if (width == entry.producer.width && height == entry.producer.height) {
                    result.success(null)
                    return
                }
                // A new size is a new surface, and the old one may be gone as soon as setSize
                // returns: the view lets go of it first.
                FynkelNative.detach(entry.view)
                entry.producer.setSize(width, height)
                val error = attach(entry)
                if (error != null) result.error("attach", error, null) else result.success(null)
            }

            "unregister" -> {
                val entry = call.argument<Number>("texture")?.toLong()?.let { live.remove(it) }
                if (entry == null) {
                    result.error("arguments", "no such texture", null)
                    return
                }
                // After this Dart frees the view.
                release(entry)
                result.success(null)
            }

            "reset" -> {
                releaseOrphans()
                result.success(null)
            }

            else -> result.notImplemented()
        }
    }

    /**
     * Hands the producer's current surface to the view, trying twice: a surface can fail once while
     * it's being made. Null, or why it failed.
     */
    private fun attach(entry: Entry): String? {
        val once =
            FynkelNative.attach(entry.view, entry.producer.surface, entry.producer.width, entry.producer.height, false)
                ?: return null
        Log.w(TAG, "couldn't draw to the surface, trying again: $once")
        return FynkelNative.attach(entry.view, entry.producer.surface, entry.producer.width, entry.producer.height, false)
    }

    /**
     * The order matters: no more lifecycle calls, the view lets go of the surface (after the frame
     * in progress), then Flutter forgets the texture. Only then may the view be freed.
     */
    private fun release(entry: Entry) {
        entry.producer.setCallback(null)
        FynkelNative.detach(entry.view)
        entry.producer.release()
    }

    /** Releases and frees every view: their isolate is gone, so no Dart code will. */
    private fun releaseOrphans() {
        for (entry in onScreen.values) {
            entry.hide()
            FynkelNative.free(entry.view)
        }
        onScreen.clear()
        for (entry in live.values) {
            release(entry)
            FynkelNative.free(entry.view)
        }
        live.clear()
    }

    private companion object {
        const val TAG = "fynkel_flutter"

        /**
         * Whether a trim level says the app is short of memory: running low or critical while in
         * front, or on the list to be killed while away. Not `TRIM_MEMORY_UI_HIDDEN`, which only says
         * the app went away, nor `TRIM_MEMORY_RUNNING_MODERATE`, which Flutter skips too.
         */
        @Suppress("DEPRECATION")
        fun warns(level: Int): Boolean =
            level >= ComponentCallbacks2.TRIM_MEMORY_RUNNING_LOW && level != ComponentCallbacks2.TRIM_MEMORY_UI_HIDDEN
    }
}

/** The engine library's JNI functions (crates/fynkel_ffi/src/jni.rs). */
internal object FynkelNative {
    init {
        // The library Dart loaded already (package:fynkel's code asset); this lets the JVM find
        // these functions in it.
        System.loadLibrary("fynkel_ffi")
    }

    /**
     * The view's next frames go to [surface], at [width] × [height]: a `SurfaceView`'s when [onScreen],
     * which Android shows itself, else a texture's, which Flutter shows. Null, or why it failed.
     */
    @JvmStatic external fun attach(view: Long, surface: android.view.Surface, width: Int, height: Int, onScreen: Boolean): String?

    /** Frames stop going to the surface, after the one in progress. */
    @JvmStatic external fun detach(view: Long)

    /** `fy_view_free`. */
    @JvmStatic external fun free(view: Long)

    /** Keep the GPU's compiled pipelines in [dir], for devices made after this. */
    @JvmStatic external fun cacheDir(dir: String)
}
