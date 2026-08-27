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
import androidx.compose.ui.unit.sp
import androidx.compose.ui.text.font.FontWeight
import androidx.core.view.WindowCompat
import androidx.core.graphics.drawable.toDrawable
import io.github.yzard.momento.core.data.ThemePreference

private val lightColors = lightColorScheme(
    primary = Color(0xFF315D3D),
    onPrimary = Color(0xFFFFFFFF),
    primaryContainer = Color(0xFFD3E8D6),
    onPrimaryContainer = Color(0xFF15361F),
    secondary = Color(0xFF5A6658),
    tertiary = Color(0xFF735A20),
    background = Color(0xFFF8F9F6),
    onBackground = Color(0xFF191C19),
    surface = Color(0xFFF8F9F6),
    onSurface = Color(0xFF191C19),
    surfaceVariant = Color(0xFFE2E5DF),
    onSurfaceVariant = Color(0xFF444843),
    outline = Color(0xFF747873),
    outlineVariant = Color(0xFFC4C8C2),
)

private val darkColors = darkColorScheme(
    primary = Color(0xFF9ECFA8),
    onPrimary = Color(0xFF07391A),
    primaryContainer = Color(0xFF1D4F2B),
    onPrimaryContainer = Color(0xFFD3E8D6),
    secondary = Color(0xFFBECBB9),
    tertiary = Color(0xFFE2C46F),
    background = Color(0xFF101410),
    onBackground = Color(0xFFE1E4DE),
    surface = Color(0xFF101410),
    onSurface = Color(0xFFE1E4DE),
    surfaceVariant = Color(0xFF292E29),
    onSurfaceVariant = Color(0xFFC3C8C1),
    outline = Color(0xFF8E938D),
    outlineVariant = Color(0xFF444943),
)

private val defaultTypography = Typography()
private val momentoTypography = Typography(
    displaySmall = defaultTypography.displaySmall.copy(
        fontWeight = FontWeight.SemiBold,
        lineHeight = 46.sp,
        letterSpacing = (-0.5).sp,
    ),
    headlineLarge = defaultTypography.headlineLarge.copy(
        fontWeight = FontWeight.SemiBold,
        lineHeight = 38.sp,
        letterSpacing = (-0.3).sp,
    ),
    headlineSmall = defaultTypography.headlineSmall.copy(fontWeight = FontWeight.SemiBold),
    titleLarge = defaultTypography.titleLarge.copy(fontWeight = FontWeight.SemiBold),
    titleMedium = defaultTypography.titleMedium.copy(fontWeight = FontWeight.Medium),
    bodyLarge = defaultTypography.bodyLarge.copy(lineHeight = 24.sp),
    bodyMedium = defaultTypography.bodyMedium.copy(lineHeight = 21.sp),
    labelLarge = defaultTypography.labelLarge.copy(fontWeight = FontWeight.Medium),
)

data class FloatingControlColors(val container: Color, val content: Color, val selected: Color)

val LocalMomentoDarkTheme = staticCompositionLocalOf { false }

fun momentoFloatingControlColors(darkTheme: Boolean): FloatingControlColors = FloatingControlColors(
    container = if (darkTheme) Color(0xFF202420).copy(alpha = 0.94f) else Color(0xFFF3F5F0).copy(alpha = 0.94f),
    content = if (darkTheme) Color(0xFFF4F6F1) else Color(0xFF202420),
    selected = if (darkTheme) Color(0xFF9ECFA8).copy(alpha = 0.24f) else Color(0xFF315D3D).copy(alpha = 0.14f),
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
                (if (darkTheme) 0xFF101410.toInt() else 0xFFF8F9F6.toInt()).toDrawable(),
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
            typography = momentoTypography,
            content = content,
        )
    }
}
