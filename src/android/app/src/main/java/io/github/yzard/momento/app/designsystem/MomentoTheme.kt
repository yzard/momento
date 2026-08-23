package io.github.yzard.momento.app.designsystem

import android.app.Activity
import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Typography
import androidx.compose.material3.darkColorScheme
import androidx.compose.material3.lightColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.SideEffect
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalView
import androidx.core.view.WindowCompat
import io.github.yzard.momento.core.data.ThemePreference

private val lightColors = lightColorScheme(
    primary = Color(0xFF315D3D),
    secondary = Color(0xFF5A6658),
    tertiary = Color(0xFF735A20),
    surface = Color(0xFFFFFBF5),
    surfaceVariant = Color(0xFFE7E6DF),
)

private val darkColors = darkColorScheme(
    primary = Color(0xFF9ECFA8),
    secondary = Color(0xFFBECBB9),
    tertiary = Color(0xFFE2C46F),
    surface = Color(0xFF121511),
    surfaceVariant = Color(0xFF292D28),
)

@Composable
fun MomentoTheme(themePreference: ThemePreference, content: @Composable () -> Unit) {
    val darkTheme = when (themePreference) {
        ThemePreference.SYSTEM -> isSystemInDarkTheme()
        ThemePreference.LIGHT -> false
        ThemePreference.DARK -> true
    }
    val view = LocalView.current
    if (!view.isInEditMode) {
        SideEffect {
            val window = (view.context as Activity).window
            WindowCompat.getInsetsController(window, view).apply {
                isAppearanceLightStatusBars = !darkTheme
                isAppearanceLightNavigationBars = !darkTheme
            }
        }
    }
    MaterialTheme(
        colorScheme = if (darkTheme) darkColors else lightColors,
        typography = Typography(),
        content = content,
    )
}
