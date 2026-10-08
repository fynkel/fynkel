package dev.fynkel.fynkel_flutter

import android.util.Log
import io.flutter.embedding.engine.plugins.FlutterPlugin
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
 * A hot restart starts a new Dart isolate without disposing anything. The first `FynkelView` of
 * each isolate calls `reset` before it registers, and every view still here then belongs to an
 * isolate that is gone: this class releases and frees those itself.
 */
class FynkelFlutterPlugin : FlutterPlugin, MethodChannel.MethodCallHandler {
    private class Entry(val view: Long, val producer: TextureRegistry.SurfaceProducer)

    private lateinit var channel: MethodChannel
    private lateinit var textures: TextureRegistry
    private val live = HashMap<Long, Entry>()

    override fun onAttachedToEngine(binding: FlutterPlugin.FlutterPluginBinding) {
        textures = binding.textureRegistry
        channel = MethodChannel(binding.binaryMessenger, "fynkel_flutter")
        channel.setMethodCallHandler(this)
    }

    override fun onDetachedFromEngine(binding: FlutterPlugin.FlutterPluginBinding) {
        channel.setMethodCallHandler(null)
        releaseOrphans()
    }

    override fun onMethodCall(call: MethodCall, result: MethodChannel.Result) {
        when (call.method) {
            "register" -> {
                val view = call.argument<Number>("view")?.toLong()
                val width = call.argument<Int>("width")
                val height = call.argument<Int>("height")
                if (view == null || view == 0L || width == null || height == null) {
                    result.error("arguments", "register needs view, width and height", null)
                    return
                }
                val producer = textures.createSurfaceProducer(TextureRegistry.SurfaceLifecycle.manual)
                val entry = Entry(view, producer)
                // Flutter 3.47 never takes a `manual` producer's surface away by itself. Should a
                // later one do so, the view lets go of it first and takes the new one.
                producer.setCallback(object : TextureRegistry.SurfaceProducer.Callback {
                    // A new surface, after one was taken away.
                    override fun onSurfaceAvailable() {
                        Log.i(TAG, "texture ${producer.id()}: a new surface")
                        attach(entry)?.let { Log.e(TAG, "couldn't draw to the new surface: $it") }
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

    /** Hands the producer's current surface to the view. Null, or why it failed. */
    private fun attach(entry: Entry): String? =
        FynkelNative.attach(entry.view, entry.producer.surface, entry.producer.width, entry.producer.height)

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
        for (entry in live.values) {
            release(entry)
            FynkelNative.free(entry.view)
        }
        live.clear()
    }

    private companion object {
        const val TAG = "fynkel_flutter"
    }
}

/** The engine library's JNI functions (crates/fynkel_ffi/src/jni.rs). */
internal object FynkelNative {
    init {
        // The library Dart loaded already (package:fynkel's code asset); this lets the JVM find
        // these functions in it.
        System.loadLibrary("fynkel_ffi")
    }

    /** The view's next frames go to [surface], at [width] × [height]. Null, or why it failed. */
    @JvmStatic external fun attach(view: Long, surface: android.view.Surface, width: Int, height: Int): String?

    /** Frames stop going to the surface, after the one in progress. */
    @JvmStatic external fun detach(view: Long)

    /** `fy_view_free`. */
    @JvmStatic external fun free(view: Long)
}
