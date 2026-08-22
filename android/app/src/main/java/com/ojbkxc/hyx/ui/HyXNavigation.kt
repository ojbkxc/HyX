package com.ojbkxc.hyx.ui

import androidx.compose.foundation.layout.padding
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.outlined.DeviceHub
import androidx.compose.material.icons.outlined.History
import androidx.compose.material.icons.outlined.SwapVert
import androidx.compose.material3.Icon
import androidx.compose.material3.NavigationBar
import androidx.compose.material3.NavigationBarItem
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
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
import com.ojbkxc.hyx.ui.screens.DevicesScreen
import com.ojbkxc.hyx.ui.screens.HistoryScreen
import com.ojbkxc.hyx.ui.screens.TransferScreen

enum class HyXTab(val route: String, val icon: ImageVector, val labelRes: Int) {
    Transfer("transfer", Icons.Outlined.SwapVert, R.string.nav_transfer),
    Devices("devices", Icons.Outlined.DeviceHub, R.string.nav_devices),
    History("history", Icons.Outlined.History, R.string.nav_history)
}

@Composable
fun HyXNavigation(controller: HyXCoreController, onScanQr: () -> Unit, onEnterCode: (String) -> Unit) {
    val nav = rememberNavController()
    val backStack by nav.currentBackStackEntryAsState()
    val currentDestination = backStack?.destination

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
            startDestination = HyXTab.Transfer.route,
            modifier = Modifier.padding(padding)
        ) {
            composable(HyXTab.Transfer.route) {
                TransferScreen(controller, onScan = onScanQr, onEnterCode = onEnterCode)
            }
            composable(HyXTab.Devices.route) { DevicesScreen(controller) }
            composable(HyXTab.History.route) { HistoryScreen(controller) }
        }
    }
}