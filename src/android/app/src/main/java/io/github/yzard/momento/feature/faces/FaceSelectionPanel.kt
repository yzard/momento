package io.github.yzard.momento.feature.faces

import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import io.github.yzard.momento.app.designsystem.MomentoSelectionAction
import io.github.yzard.momento.app.designsystem.MomentoSelectionDock
import io.github.yzard.momento.app.designsystem.PublishSelectionDock

@Composable
internal fun FaceSelectionPanel(
    selectedIds: List<Long>,
    working: Boolean,
    modifier: Modifier,
    actions: List<MomentoSelectionAction>,
    clearSelection: () -> Unit,
) {
    PublishSelectionDock {
        MomentoSelectionDock(selectedIds.size, actions, clearSelection, !working, modifier)
    }
}
