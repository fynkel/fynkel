package dev.fynkel.lab

import android.os.Bundle
import android.view.WindowManager
import dev.fynkel.fynkel_flutter.FynkelActivity

class MainActivity : FynkelActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        // A measurement runs for minutes untouched: the screen must stay on.
        window.addFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON)
    }
}
