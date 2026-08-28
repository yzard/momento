package io.github.yzard.momento.app.designsystem

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Close
import androidx.compose.material.icons.filled.CheckCircle
import androidx.compose.material.icons.filled.RadioButtonUnchecked
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.ButtonDefaults
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.unit.dp

@Composable
fun MomentoSelectionMark(
    selected: Boolean,
    contentDescription: String,
    modifier: Modifier,
) {
    Icon(
        imageVector = if (selected) Icons.Default.CheckCircle else Icons.Default.RadioButtonUnchecked,
        contentDescription = contentDescription,
        tint = if (selected) Color.White else Color.White.copy(alpha = 0.82f),
        modifier = modifier,
    )
}

data class MomentoSelectionAction(
    val label: String,
    val icon: ImageVector,
    val enabled: Boolean,
    val destructive: Boolean,
    val perform: () -> Unit,
)

fun momentoSelectionCountLabel(selectedCount: Int): String {
    require(selectedCount > 0) { "Selection count must be positive" }
    return "$selectedCount selected"
}

@Composable
fun MomentoSelectionDock(
    selectedCount: Int,
    actions: List<MomentoSelectionAction>,
    clearSelection: () -> Unit,
    modifier: Modifier,
) {
    val colors = momentoFloatingControlColors()
    MomentoFloatingDock(modifier) {
        Row(
            modifier = Modifier.padding(start = 10.dp),
            horizontalArrangement = Arrangement.spacedBy(2.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Text(momentoSelectionCountLabel(selectedCount))
            Spacer(Modifier.width(2.dp))
            actions.forEach { action ->
                TextButton(
                    onClick = action.perform,
                    enabled = action.enabled,
                    colors = ButtonDefaults.textButtonColors(
                        contentColor = if (action.destructive) Color(0xFFFF453A) else colors.content,
                    ),
                ) {
                    Icon(action.icon, contentDescription = null)
                    Spacer(Modifier.width(6.dp))
                    Text(action.label)
                }
            }
            IconButton(onClick = clearSelection) {
                Icon(Icons.Default.Close, "Clear selection")
            }
        }
    }
}
