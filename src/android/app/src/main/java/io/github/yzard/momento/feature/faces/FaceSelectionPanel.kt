package io.github.yzard.momento.feature.faces

import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.lazy.LazyRow
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Check
import androidx.compose.material.icons.filled.Close
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import io.github.yzard.momento.app.designsystem.MomentoSelectionAction
import io.github.yzard.momento.app.designsystem.MomentoSelectionActionButton
import io.github.yzard.momento.app.designsystem.momentoFloatingControlColors
import io.github.yzard.momento.app.designsystem.momentoSelectionCountLabel

@Composable
internal fun FaceSelectionPanel(
    selectedIds: List<Long>,
    working: Boolean,
    actions: List<MomentoSelectionAction>,
    removeSelection: (Long) -> Unit,
    clearSelection: () -> Unit,
    thumbnail: @Composable (Long, Modifier) -> Unit,
    modifier: Modifier,
) {
    val colors = momentoFloatingControlColors()
    Surface(
        modifier = modifier.widthIn(max = 460.dp).fillMaxWidth(),
        shape = RoundedCornerShape(28.dp),
        color = colors.container,
        contentColor = colors.content,
        border = BorderStroke(1.dp, colors.outline),
        shadowElevation = 12.dp,
    ) {
        Column(Modifier.padding(horizontal = 12.dp, vertical = 6.dp)) {
            Row(verticalAlignment = Alignment.CenterVertically) {
                Text(
                    momentoSelectionCountLabel(selectedIds.size),
                    modifier = Modifier.weight(1f).padding(start = 6.dp),
                    style = MaterialTheme.typography.titleSmall,
                    fontWeight = FontWeight.SemiBold,
                )
                IconButton(enabled = !working, onClick = clearSelection) {
                    Icon(Icons.Default.Close, "Clear selection", modifier = Modifier.size(20.dp))
                }
            }
            LazyRow(
                modifier = Modifier.fillMaxWidth().padding(bottom = 10.dp),
                horizontalArrangement = Arrangement.spacedBy(8.dp),
            ) {
                items(selectedIds, key = { it }) { id ->
                    Box(
                        Modifier.size(56.dp).clip(RoundedCornerShape(16.dp))
                            .background(colors.selected)
                            .border(1.dp, colors.outline, RoundedCornerShape(16.dp))
                            .clickable(enabled = !working, role = Role.Button) { removeSelection(id) }
                            .semantics(mergeDescendants = true) { contentDescription = "Deselect face group $id" },
                    ) {
                        thumbnail(id, Modifier.matchParentSize())
                        Icon(
                            Icons.Default.Check, contentDescription = null,
                            tint = colors.container,
                            modifier = Modifier.align(Alignment.BottomEnd).padding(4.dp)
                                .background(colors.content, CircleShape).padding(2.dp).size(12.dp),
                        )
                    }
                }
            }
            HorizontalDivider(color = colors.outline)
            Row(Modifier.fillMaxWidth().padding(top = 2.dp), horizontalArrangement = Arrangement.spacedBy(4.dp)) {
                actions.forEach { action ->
                    MomentoSelectionActionButton(action, Modifier.weight(1f))
                }
            }
        }
    }
}
