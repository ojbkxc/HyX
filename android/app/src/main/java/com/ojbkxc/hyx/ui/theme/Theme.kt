package com.ojbkxc.hyx.ui.theme

import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.darkColorScheme
import androidx.compose.material3.lightColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.ui.graphics.Color

private val LightColors = lightColorScheme(
    primary = HyxGreenDark,
    onPrimary = Color.White,
    primaryContainer = HyxGreenContainer,
    onPrimaryContainer = Slate950,
    secondary = HyxGreen,
    onSecondary = Color.White,
    secondaryContainer = HyxGreenContainer,
    onSecondaryContainer = Slate950,
    background = Slate50,
    onBackground = Slate950,
    surface = Color.White,
    onSurface = Slate950,
    surfaceVariant = Slate100,
    onSurfaceVariant = Slate700,
    outline = Slate300
)

private val DarkColors = darkColorScheme(
    primary = HyxGreen,
    onPrimary = Slate950,
    primaryContainer = Slate800,
    onPrimaryContainer = HyxGreen,
    secondary = HyxGreen,
    onSecondary = Slate950,
    secondaryContainer = Slate800,
    onSecondaryContainer = HyxGreenContainer,
    background = Slate950,
    onBackground = Slate100,
    surface = Slate900,
    onSurface = Slate100,
    surfaceVariant = Slate800,
    onSurfaceVariant = Slate300,
    outline = Slate700
)

@Composable
fun HyXTheme(darkTheme: Boolean = isSystemInDarkTheme(), content: @Composable () -> Unit) {
    MaterialTheme(
        colorScheme = if (darkTheme) DarkColors else LightColors,
        typography = HyXTypography,
        shapes = HyXShapes,
        content = content
    )
}