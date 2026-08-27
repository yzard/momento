package io.github.yzard.momento.app.designsystem

import androidx.compose.animation.animateColorAsState
import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.clickable
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.interaction.collectIsPressedAsState
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.RowScope
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.statusBars
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.windowInsetsPadding
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.compositeOver
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp

val momentoMediaViewerContentPadding = PaddingValues(top = 104.dp, bottom = 104.dp)

@Composable
fun MomentoPageTitle(text: String, modifier: Modifier) {
    Text(
        text = text,
        modifier = modifier,
        color = MaterialTheme.colorScheme.onBackground,
        style = MaterialTheme.typography.headlineLarge,
        fontWeight = FontWeight.SemiBold,
        maxLines = 1,
        overflow = TextOverflow.Ellipsis,
    )
}

@Composable
fun MomentoMediaPageTitle(text: String, modifier: Modifier) {
    val colors = momentoFloatingControlColors(darkTheme = true)
    Surface(
        modifier = modifier
            .padding(start = 12.dp, top = 10.dp),
        shape = MaterialTheme.shapes.extraLarge,
        color = colors.container,
        contentColor = colors.content,
        border = BorderStroke(1.dp, colors.outline),
        shadowElevation = 0.dp,
        tonalElevation = 0.dp,
    ) {
        Text(
            text = text,
            modifier = Modifier.heightIn(min = 48.dp).padding(horizontal = 16.dp, vertical = 8.dp),
            style = MaterialTheme.typography.headlineSmall,
            fontWeight = FontWeight.SemiBold,
            maxLines = 1,
            overflow = TextOverflow.Ellipsis,
        )
    }
}

@Composable
fun MomentoPageHeader(
    title: String,
    subtitle: String?,
    modifier: Modifier,
    leadingContent: (@Composable RowScope.() -> Unit)?,
    trailingContent: (@Composable RowScope.() -> Unit)?,
) {
    Row(
        modifier = modifier.fillMaxWidth().heightIn(min = 64.dp).padding(horizontal = 20.dp, vertical = 8.dp),
        horizontalArrangement = Arrangement.Start,
        verticalAlignment = Alignment.CenterVertically,
    ) {
        if (leadingContent != null) {
            leadingContent()
            Spacer(Modifier.width(4.dp))
        }
        Column(Modifier.weight(1f)) {
            MomentoPageTitle(title, Modifier.fillMaxWidth())
            if (subtitle != null) {
                Text(
                    text = subtitle,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    style = MaterialTheme.typography.labelMedium,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                )
            }
        }
        if (trailingContent != null) {
            Spacer(Modifier.width(12.dp))
            trailingContent()
        }
    }
}

@Composable
fun MomentoDetailPageHeader(
    title: String,
    subtitle: String?,
    backContentDescription: String,
    enabled: Boolean,
    onBack: () -> Unit,
    modifier: Modifier,
) {
    val colors = momentoFloatingControlColors(darkTheme = true)
    Surface(
        modifier = modifier
            .windowInsetsPadding(WindowInsets.statusBars)
            .padding(start = 12.dp, top = 10.dp, end = 12.dp),
        shape = MaterialTheme.shapes.extraLarge,
        color = colors.container,
        contentColor = colors.content,
        border = BorderStroke(1.dp, colors.outline),
        shadowElevation = 0.dp,
        tonalElevation = 0.dp,
    ) {
        Row(
            modifier = Modifier.heightIn(min = 56.dp).padding(end = 16.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            IconButton(onClick = onBack, enabled = enabled) {
                Icon(Icons.AutoMirrored.Filled.ArrowBack, backContentDescription)
            }
            Column(Modifier.weight(1f)) {
                Text(
                    text = title,
                    style = MaterialTheme.typography.titleLarge,
                    fontWeight = FontWeight.SemiBold,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                )
                if (subtitle != null) {
                    Text(
                        text = subtitle,
                        color = colors.content.copy(alpha = 0.72f),
                        style = MaterialTheme.typography.labelMedium,
                        maxLines = 1,
                        overflow = TextOverflow.Ellipsis,
                    )
                }
            }
        }
    }
}

@Composable
fun MomentoFloatingDock(
    modifier: Modifier,
    content: @Composable RowScope.() -> Unit,
) {
    val colors = momentoFloatingControlColors()
    FloatingSurface(modifier, colors.container, colors) {
        Row(
            modifier = Modifier.padding(4.dp),
            horizontalArrangement = Arrangement.spacedBy(2.dp),
            verticalAlignment = Alignment.CenterVertically,
            content = content,
        )
    }
}

@Composable
fun MomentoActionChip(
    label: String,
    icon: ImageVector,
    enabled: Boolean,
    onClick: () -> Unit,
    modifier: Modifier,
) {
    val colors = momentoFloatingControlColors()
    val interactionSource = remember { MutableInteractionSource() }
    val pressed by interactionSource.collectIsPressedAsState()
    val targetContainer = momentoActionChipContainerColor(colors, pressed, enabled)
    val container by animateColorAsState(targetValue = targetContainer, label = "Momento action chip")
    FloatingSurface(
        modifier = modifier
            .heightIn(min = 48.dp)
            .clickable(
                interactionSource = interactionSource,
                indication = null,
                enabled = enabled,
                role = Role.Button,
                onClick = onClick,
            ),
        container = container,
        colors = colors,
    ) {
        Row(
            modifier = Modifier.padding(horizontal = 14.dp, vertical = 10.dp),
            horizontalArrangement = Arrangement.spacedBy(7.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Icon(icon, contentDescription = null, modifier = Modifier.size(18.dp))
            Text(label, style = MaterialTheme.typography.labelLarge, maxLines = 1)
        }
    }
}

internal fun momentoActionChipContainerColor(
    colors: FloatingControlColors,
    pressed: Boolean,
    enabled: Boolean,
): Color {
    if (!enabled) return colors.container.copy(alpha = colors.container.alpha * 0.55f)
    if (!pressed) return colors.container
    return colors.selected.compositeOver(colors.container)
}

@Composable
private fun FloatingSurface(
    modifier: Modifier,
    container: Color,
    colors: FloatingControlColors,
    content: @Composable () -> Unit,
) {
    Surface(
        modifier = modifier,
        shape = MaterialTheme.shapes.extraLarge,
        color = container,
        contentColor = colors.content,
        border = BorderStroke(1.dp, colors.outline),
        shadowElevation = 0.dp,
        tonalElevation = 0.dp,
        content = content,
    )
}
