package io.github.yzard.momento.app.designsystem

import android.app.Activity
import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxScope
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Shapes
import androidx.compose.material3.Surface
import androidx.compose.material3.Typography
import androidx.compose.material3.ColorScheme
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
    primary = Color(0xFF000000),
    onPrimary = Color(0xFFFFFFFF),
    primaryContainer = Color(0xFFE5E5EA),
    onPrimaryContainer = Color(0xFF000000),
    secondary = Color(0xFF3A3A3C),
    onSecondary = Color(0xFFFFFFFF),
    secondaryContainer = Color(0xFFE5E5EA),
    onSecondaryContainer = Color(0xFF000000),
    tertiary = Color(0xFF000000),
    onTertiary = Color(0xFFFFFFFF),
    tertiaryContainer = Color(0xFFF2F2F7),
    onTertiaryContainer = Color(0xFF000000),
    error = Color(0xFFFF3B30),
    onError = Color(0xFF000000),
    errorContainer = Color(0xFFFFE5E5),
    onErrorContainer = Color(0xFF7A0000),
    background = Color(0xFFFFFFFF),
    onBackground = Color(0xFF000000),
    surface = Color(0xFFFFFFFF),
    onSurface = Color(0xFF000000),
    surfaceVariant = Color(0xFFF2F2F7),
    onSurfaceVariant = Color(0xFF6E6E73),
    outline = Color(0xFF8E8E93),
    outlineVariant = Color(0xFFD1D1D6),
    scrim = Color(0xFF000000),
    inverseSurface = Color(0xFF1C1C1E),
    inverseOnSurface = Color(0xFFF2F2F7),
    inversePrimary = Color(0xFFFFFFFF),
    surfaceDim = Color(0xFFE5E5EA),
    surfaceBright = Color(0xFFFFFFFF),
    surfaceContainerLowest = Color(0xFFFFFFFF),
    surfaceContainerLow = Color(0xFFF7F7F8),
    surfaceContainer = Color(0xFFF2F2F7),
    surfaceContainerHigh = Color(0xFFEAEAEE),
    surfaceContainerHighest = Color(0xFFE5E5EA),
    surfaceTint = Color.Transparent,
)

private val darkColors = darkColorScheme(
    primary = Color(0xFFFFFFFF),
    onPrimary = Color(0xFF000000),
    primaryContainer = Color(0xFF2C2C2E),
    onPrimaryContainer = Color(0xFFFFFFFF),
    secondary = Color(0xFFD1D1D6),
    onSecondary = Color(0xFF1C1C1E),
    secondaryContainer = Color(0xFF2C2C2E),
    onSecondaryContainer = Color(0xFFF2F2F7),
    tertiary = Color(0xFFF2F2F7),
    onTertiary = Color(0xFF000000),
    tertiaryContainer = Color(0xFF2C2C2E),
    onTertiaryContainer = Color(0xFFFFFFFF),
    error = Color(0xFFFF453A),
    onError = Color(0xFF000000),
    errorContainer = Color(0xFF5C1815),
    onErrorContainer = Color(0xFFFFDAD6),
    background = Color(0xFF000000),
    onBackground = Color(0xFFFFFFFF),
    surface = Color(0xFF000000),
    onSurface = Color(0xFFFFFFFF),
    surfaceVariant = Color(0xFF1C1C1E),
    onSurfaceVariant = Color(0xFFA1A1A6),
    outline = Color(0xFF636366),
    outlineVariant = Color(0xFF38383A),
    scrim = Color(0xFF000000),
    inverseSurface = Color(0xFFF2F2F7),
    inverseOnSurface = Color(0xFF1C1C1E),
    inversePrimary = Color(0xFF000000),
    surfaceDim = Color(0xFF000000),
    surfaceBright = Color(0xFF2C2C2E),
    surfaceContainerLowest = Color(0xFF000000),
    surfaceContainerLow = Color(0xFF0A0A0A),
    surfaceContainer = Color(0xFF111111),
    surfaceContainerHigh = Color(0xFF1C1C1E),
    surfaceContainerHighest = Color(0xFF2C2C2E),
    surfaceTint = Color.Transparent,
)

internal fun momentoColorScheme(darkTheme: Boolean): ColorScheme = if (darkTheme) darkColors else lightColors

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

private val momentoShapes = Shapes(
    small = RoundedCornerShape(10.dp),
    medium = RoundedCornerShape(14.dp),
    large = RoundedCornerShape(18.dp),
    extraLarge = RoundedCornerShape(28.dp),
)

data class FloatingControlColors(
    val container: Color,
    val content: Color,
    val selected: Color,
    val outline: Color,
)

val LocalMomentoDarkTheme = staticCompositionLocalOf { false }

fun momentoFloatingControlColors(darkTheme: Boolean): FloatingControlColors = FloatingControlColors(
    container = if (darkTheme) Color(0xFF1C1C1E).copy(alpha = 0.94f) else Color(0xFFF5F5F7).copy(alpha = 0.96f),
    content = if (darkTheme) Color.White else Color.Black,
    selected = if (darkTheme) Color.White.copy(alpha = 0.16f) else Color.Black.copy(alpha = 0.10f),
    outline = if (darkTheme) Color.White.copy(alpha = 0.12f) else Color.Black.copy(alpha = 0.08f),
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
        border = BorderStroke(1.dp, colors.outline),
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
                (if (darkTheme) 0xFF000000.toInt() else 0xFFFFFFFF.toInt()).toDrawable(),
            )
            WindowCompat.getInsetsController(window, view).apply {
                isAppearanceLightStatusBars = !darkTheme
                isAppearanceLightNavigationBars = !darkTheme
            }
        }
    }
    CompositionLocalProvider(LocalMomentoDarkTheme provides darkTheme) {
        MaterialTheme(
            colorScheme = momentoColorScheme(darkTheme),
            typography = momentoTypography,
            shapes = momentoShapes,
            content = content,
        )
    }
}
