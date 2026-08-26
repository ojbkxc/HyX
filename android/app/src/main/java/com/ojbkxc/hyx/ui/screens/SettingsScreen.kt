package com.ojbkxc.hyx.ui.screens

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material3.Button
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.material3.TopAppBar
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import com.ojbkxc.hyx.core.HyXCoreController

/**
 * 设置页 —— 对齐 Flutter full version 的设置页面设计。
 *
 * 当前仅包含自定义设备名称设置：用户输入名称 → 保存 → 调 JNI 同步到 Rust 侧 →
 * 持久化到 SharedPreferences。beacon 会携带新的 device_name，对端 discover 后
 * 自动显示。留空则使用默认名称（Rust 侧重置）。
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun SettingsScreen(controller: HyXCoreController, onBack: () -> Unit) {
    var name by remember { mutableStateOf(controller.getCustomName()) }
    Scaffold(
        topBar = {
            TopAppBar(
                title = { Text("设置") },
                navigationIcon = {
                    IconButton(onClick = onBack) {
                        Icon(
                            Icons.AutoMirrored.Filled.ArrowBack,
                            contentDescription = "返回"
                        )
                    }
                }
            )
        }
    ) { padding ->
        Column(
            modifier = Modifier.padding(padding).padding(16.dp).fillMaxWidth(),
            verticalArrangement = Arrangement.spacedBy(16.dp)
        ) {
            Text("设备名称", style = MaterialTheme.typography.titleMedium)
            Text(
                "此名称将显示在其它设备的设备列表中。留空则使用默认名称。",
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant
            )
            OutlinedTextField(
                value = name,
                onValueChange = { name = it },
                label = { Text("自定义名称") },
                modifier = Modifier.fillMaxWidth(),
                singleLine = true
            )
            Button(
                onClick = {
                    controller.setCustomName(name)
                    onBack()
                },
                modifier = Modifier.fillMaxWidth()
            ) {
                Text("保存")
            }
        }
    }
}