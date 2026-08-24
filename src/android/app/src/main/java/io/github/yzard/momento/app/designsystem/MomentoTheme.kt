package io.github.yzard.momento.app.designsystem

import android.app.Activity
import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxScope
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Typography
import androidx.compose.material3.darkColorScheme
import androidx.compose.material3.lightColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.SideEffect
import androidx.compose.runtime.staticCompositionLocalOf
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalView
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.unit.dp
import androidx.core.view.WindowCompat
import androidx.core.graphics.drawable.toDrawable
import io.github.yzard.momento.core.data.ThemePreference

private val lightColors = lightColorScheme(
    primary = Color(0xFF315D3D),
    secondary = Color(0xFF5A6658),
    tertiary = Color(0xFF735A20),
    background = Color.White,
    onBackground = Color.Black,
    surface = Color.White,
    onSurface = Color.Black,
    surfaceVariant = Color(0xFFE5E5E5),
    onSurfaceVariant = Color(0xFF202020),
)

private val darkColors = darkColorScheme(
    primary = Color(0xFF9ECFA8),
    secondary = Color(0xFFBECBB9),
    tertiary = Color(0xFFE2C46F),
    background = Color.Black,
    onBackground = Color.White,
    surface = Color.Black,
    onSurface = Color.White,
    surfaceVariant = Color(0xFF222222),
    onSurfaceVariant = Color(0xFFE5E5E5),
)

data class FloatingControlColors(val container: Color, val content: Color, val selected: Color)

val LocalMomentoDarkTheme = staticCompositionLocalOf { false }

fun momentoFloatingControlColors(darkTheme: Boolean): FloatingControlColors = FloatingControlColors(
    container = if (darkTheme) Color(0xFF555555).copy(alpha = 0.76f) else Color(0xFFB8B8B8).copy(alpha = 0.76f),
    content = if (darkTheme) Color.White else Color.Black,
    selected = if (darkTheme) Color.White.copy(alpha = 0.2f) else Color.Black.copy(alpha = 0.14f),
)

@Composable
fun momentoFloatingControlColors(): FloatingControlColors =
    momentoFloatingControlColors(LocalMomentoDarkTheme.current)

@Composable
fun MomentoFloatingButton(
    modifier: Modifier,
    onClick: () -> Unit,
    content: @Composable BoxScope.() -> Unit,
) {
    val colors = momentoFloatingControlColors()
    Surface(
        modifier = modifier.size(56.dp),
        shape = CircleShape,
        color = colors.container,
        contentColor = colors.content,
        shadowElevation = 0.dp,
        tonalElevation = 0.dp,
    ) {
        Box(
            modifier = Modifier.fillMaxSize().clickable(role = Role.Button, onClick = onClick),
            contentAlignment = Alignment.Center,
            content = content,
        )
    }
}

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
            window.setBackgroundDrawable(
                (if (darkTheme) android.graphics.Color.BLACK else android.graphics.Color.WHITE).toDrawable(),
            )
            WindowCompat.getInsetsController(window, view).apply {
                isAppearanceLightStatusBars = !darkTheme
                isAppearanceLightNavigationBars = !darkTheme
            }
        }
    }
    CompositionLocalProvider(LocalMomentoDarkTheme provides darkTheme) {
        MaterialTheme(
            colorScheme = if (darkTheme) darkColors else lightColors,
            typography = Typography(),
            content = content,
        )
    }
}
