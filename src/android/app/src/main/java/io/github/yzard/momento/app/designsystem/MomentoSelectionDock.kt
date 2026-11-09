package io.github.yzard.momento.app.designsystem

import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.size
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Close
import androidx.compose.material.icons.filled.Check
import androidx.compose.material.icons.filled.RadioButtonUnchecked
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.ButtonDefaults
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.unit.dp

@Composable
fun MomentoSelectionMark(
    selected: Boolean,
    contentDescription: String,
    modifier: Modifier,
) {
    androidx.compose.foundation.layout.Box(modifier.size(24.dp).semantics { this.contentDescription = contentDescription }, contentAlignment = Alignment.Center) {
        androidx.compose.foundation.Canvas(Modifier.matchParentSize()) {
            if (selected) drawCircle(Color.Black.copy(alpha = 0.75f))
            drawCircle(Color.White, radius = (size.minDimension - 2.dp.toPx()) / 2, style = androidx.compose.ui.graphics.drawscope.Stroke(width = 2.dp.toPx()))
        }
        if (selected) Icon(Icons.Default.Check, null, tint = Color.White, modifier = Modifier.size(16.dp))
    }
}

data class MomentoSelectionAction(
    val label: String,
    val icon: ImageVector,
    val enabled: Boolean,
    val destructive: Boolean,
    val perform: () -> Unit,
)

fun momentoSelectionCountLabel(selectedCount: Int): String {
    require(selectedCount >= 0) { "Selection count must not be negative" }
    return "$selectedCount selected"
}

@Composable
fun MomentoSelectionDock(
    selectedCount: Int,
    actions: List<MomentoSelectionAction>,
    clearSelection: () -> Unit,
    clearEnabled: Boolean,
    modifier: Modifier,
) {
    MomentoFloatingDock(modifier) {
        Row(
            modifier = Modifier.padding(start = 10.dp).horizontalScroll(rememberScrollState()),
            horizontalArrangement = Arrangement.spacedBy(2.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Text(momentoSelectionCountLabel(selectedCount))
            Spacer(Modifier.width(2.dp))
            actions.forEach { action ->
                MomentoSelectionActionButton(action, Modifier)
            }
            IconButton(onClick = clearSelection, enabled = clearEnabled) {
                Icon(Icons.Default.Close, "Clear selection")
            }
        }
    }
}

@Composable
fun MomentoSelectionActionButton(action: MomentoSelectionAction, modifier: Modifier) {
    val colors = momentoFloatingControlColors()
    TextButton(
        onClick = action.perform,
        enabled = action.enabled,
        modifier = modifier,
        colors = ButtonDefaults.textButtonColors(
            contentColor = if (action.destructive) Color(0xFFFF453A) else colors.content,
        ),
    ) {
        Icon(action.icon, contentDescription = null)
        Spacer(Modifier.width(6.dp))
        Text(action.label)
    }
}


class SelectionDockState {
    var content by androidx.compose.runtime.mutableStateOf<(@Composable () -> Unit)?>(null)
    var owner: Any? = null
}

val LocalSelectionDock = androidx.compose.runtime.staticCompositionLocalOf<SelectionDockState?> { null }

@Composable
fun PublishSelectionDock(content: @Composable () -> Unit) {
    val state = LocalSelectionDock.current
    if (state == null) { content(); return }
    val owner = androidx.compose.runtime.remember { Any() }
    androidx.compose.runtime.SideEffect { state.owner = owner; state.content = content }
    androidx.compose.runtime.DisposableEffect(state) {
        onDispose { if (state.owner === owner) { state.owner = null; state.content = null } }
    }
}
