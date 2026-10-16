package dev.fynkel.fynkel_flutter

import io.flutter.embedding.android.FlutterActivity
import io.flutter.embedding.android.FlutterActivityLaunchConfigs.BackgroundMode
import io.flutter.embedding.android.RenderMode

/**
 * A game's activity: Flutter's view is see-through, so each `FynkelView` can draw straight to the
 * screen, to a `SurfaceView` behind it, and Flutter never waits for the game's frames
 * (docs/android-surface.md). A game's `MainActivity` extends this instead of `FlutterActivity`.
 *
 * Flutter keeps a surface of its own: a see-through `FlutterActivity` would otherwise draw through
 * a `TextureView`, which Android composites on the GPU every frame.
 */
open class FynkelActivity : FlutterActivity() {
    override fun getBackgroundMode(): BackgroundMode = BackgroundMode.transparent

    override fun getRenderMode(): RenderMode = RenderMode.surface
}
