package com.ojbkxc.hyx

import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.activity.viewModels
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.ui.Modifier
import com.journeyapps.barcodescanner.ScanContract
import com.journeyapps.barcodescanner.ScanOptions
import com.ojbkxc.hyx.core.HyXCoreController
import com.ojbkxc.hyx.ui.HyXNavigation
import com.ojbkxc.hyx.ui.theme.HyXTheme

class MainActivity : ComponentActivity() {

    // Single activity-scoped ViewModel shared by all three tabs.
    private val controller: HyXCoreController by viewModels()

    // QR scan via zxing-android-embedded's ScanContract (an
    // ActivityResultContract<ScanOptions, ScanIntentResult>). The result is
    // delivered as a ScanIntentResult; result.contents is the decoded text or
    // null when the user cancels.
    private val barcodeLauncher = registerForActivityResult(ScanContract()) { result ->
        val raw = result.contents
        if (!raw.isNullOrEmpty()) controller.scanQr(raw)
        // null → user cancelled; nothing to do.
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        enableEdgeToEdge()
        setContent {
            HyXTheme {
                Surface(
                    modifier = Modifier.fillMaxSize(),
                    color = MaterialTheme.colorScheme.background
                ) {
                    HyXNavigation(
                        controller = controller,
                        onScanQr = ::launchScanner,
                        onEnterCode = { controller.pairWithCode(it) }
                    )
                }
            }
        }
    }

    /** Launch the zxing camera scanner. The result lands in [barcodeLauncher]
     *  and is routed to [HyXCoreController.scanQr]. */
    private fun launchScanner() {
        barcodeLauncher.launch(
            ScanOptions()
                .setDesiredBarcodeFormats(ScanOptions.QR_CODE)
                .setPrompt(getString(R.string.qr_scan_prompt))
                .setBeepEnabled(true)
                .setOrientationLocked(false)
        )
    }
}
