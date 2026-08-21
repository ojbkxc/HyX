package com.ojbkxc.hyx.ui.theme

import androidx.compose.material3.Typography
import androidx.compose.ui.text.font.FontWeight

val HyXTypography = Typography().run {
    copy(
        headlineSmall = androidx.compose.material3.Typography().headlineSmall.copy(fontWeight = FontWeight.Bold),
        titleMedium = androidx.compose.material3.Typography().titleMedium.copy(fontWeight = FontWeight.SemiBold),
        labelMedium = androidx.compose.material3.Typography().labelMedium.copy(fontWeight = FontWeight.Medium)
    )
}