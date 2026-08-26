package com.ojbkxc.hyx.ui

import androidx.compose.foundation.layout.padding
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.outlined.DeviceHub
import androidx.compose.material.icons.outlined.History
import androidx.compose.material3.Icon
import androidx.compose.material3.NavigationBar
import androidx.compose.material3.NavigationBarItem
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.res.stringResource
import androidx.navigation.NavDestination.Companion.hierarchy
import androidx.navigation.NavGraph.Companion.findStartDestination
import androidx.navigation.compose.NavHost
import androidx.navigation.compose.composable
import androidx.navigation.compose.currentBackStackEntryAsState
import androidx.navigation.compose.rememberNavController
import com.ojbkxc.hyx.R
import com.ojbkxc.hyx.core.HyXCoreController
import com.ojbkxc.hyx.ui.model.TransferProgress
import com.ojbkxc.hyx.ui.model.TransferStatus
import com.ojbkxc.hyx.ui.screens.DevicesScreen
import com.ojbkxc.hyx.ui.screens.HistoryScreen
import com.ojbkxc.hyx.ui.screens.SettingsScreen
import com.ojbkxc.hyx.ui.screens.TransferProgressSheet
import kotlinx.coroutines.delay

enum class HyXTab(val route: String, val icon: ImageVector, val labelRes: Int) {
    Devices("devices", Icons.Outlined.DeviceHub, R.string.nav_devices),
    History("history", Icons.Outlined.History, R.string.nav_history)
}

@Composable
fun HyXNavigation(controller: HyXCoreController) {
    val nav = rememberNavController()
    val backStack by nav.currentBackStackEntryAsState()
    val currentDestination = backStack?.destination

    // 传输进度浮层状态：监听 controller.status / progress，自动弹出/关闭。
    val status by controller.status.collectAsState()
    val progress by controller.progress.collectAsState()
    var showSheet by remember { mutableStateOf(false) }
    // 最近一次非空进度快照——终态时 controller 会清空 progress，保留此快照供展示结果。
    var snapshotProgress by remember { mutableStateOf<TransferProgress?>(null) }
    var snapshotStatus by remember { mutableStateOf(TransferStatus.Idle) }

    // 跟踪最新非空进度，供终态展示。
    LaunchedEffect(progress) {
        if (progress != null) snapshotProgress = progress
    }

    // 状态驱动：忙态弹出，终态保持 1.5s 后自动关闭。
    LaunchedEffect(status) {
        snapshotStatus = status
        when (status) {
            TransferStatus.Connecting,
            TransferStatus.Pairing,
            TransferStatus.Transferring -> {
                showSheet = true
            }
            TransferStatus.Completed,
            TransferStatus.Failed,
            TransferStatus.Cancelled -> {
                // 展示最终状态 1.5s 后自动关闭，对齐 Flutter transfer_progress_sheet 行为。
                showSheet = true
                delay(1500)
                showSheet = false
            }
            TransferStatus.Idle -> {
                showSheet = false
            }
        }
    }

    Scaffold(
        bottomBar = {
            NavigationBar {
                HyXTab.entries.forEach { tab ->
                    val selected = currentDestination?.hierarchy?.any { it.route == tab.route } == true
                    NavigationBarItem(
                        selected = selected,
                        onClick = {
                            nav.navigate(tab.route) {
                                popUpTo(nav.graph.findStartDestination().id) { saveState = true }
                                launchSingleTop = true
                                restoreState = true
                            }
                        },
                        icon = { Icon(tab.icon, contentDescription = null) },
                        label = { Text(stringResource(tab.labelRes)) }
                    )
                }
            }
        }
    ) { padding ->
        NavHost(
            navController = nav,
            startDestination = HyXTab.Devices.route,
            modifier = Modifier.padding(padding)
        ) {
            composable(HyXTab.Devices.route) {
                DevicesScreen(
                    controller,
                    onNavigateToSettings = { nav.navigate("settings") }
                )
            }
            composable(HyXTab.History.route) { HistoryScreen(controller) }
            composable("settings") {
                SettingsScreen(controller, onBack = { nav.popBackStack() })
            }
        }

        // 传输进度浮层：有快照时才显示。
        if (showSheet) {
            snapshotProgress?.let { p ->
                TransferProgressSheet(
                    progress = p,
                    status = snapshotStatus,
                    onCancel = { controller.cancelTransfer() },
                    onDismiss = { showSheet = false }
                )
            }
        }
    }
}
