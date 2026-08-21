package com.ojbkxc.hyx

import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.activity.result.contract.ActivityResultContracts
import androidx.activity.viewModels
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.ui.Modifier
import com.ojbkxc.hyx.core.HyXCoreController
import com.ojbkxc.hyx.ui.HyXNavigation
import com.ojbkxc.hyx.ui.theme.HyXTheme

class MainActivity : ComponentActivity() {

    // Single activity-scoped ViewModel shared by all three tabs.
    private val controller: HyXCoreController by viewModels()

    // QR scan via the system on-device scanner (ML Kit, no extra permission).
    private val barcodeLauncher =
        registerForActivityResult(ActivityResultContracts.StartActivityForResult()) { result ->
            val raw = result.data?.getStringExtra("SCAN_RESULT")
            if (!raw.isNullOrEmpty()) controller.scanQr(raw)
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
                    HyXNavigation(controller = controller, onScanQr = ::launchScanner)
                }
            }
        }
    }

    private fun launchScanner() {
        // Stand-in: wire an ML Kit / zxing scanner here. For a self-contained
        // build without extra deps, pair with a fixed demo code for now.
        controller.pairWithCode("HYX-4821")
    }
}