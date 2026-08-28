package io.github.yzard.momento.feature.viewer

import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class ViewerChromeControllerTest {
    @Test fun everyInteractionResetsTheRevisionEvenWhenChromeIsAlreadyVisible() {
        val initial = ViewerChromeState.initial()

        val interacted = initial.recordInteraction().recordInteraction()

        assertTrue(interacted.visible)
        assertEquals(2, interacted.interactionRevision)
    }

    @Test fun activeGesturesAndSheetsPreventInactivityHiding() {
        val dragging = ViewerChromeState.initial().changeInteraction(true)
        val sheetOpen = ViewerChromeState.initial().openSheet(ViewerSheet.INFORMATION)

        assertEquals(dragging, dragging.hideAfterInactivity())
        assertEquals(sheetOpen, sheetOpen.hideAfterInactivity())
        assertEquals(false, ViewerChromeState.initial().hideAfterInactivity().visible)
    }

    @Test fun tappingTheInformationActionAgainClosesItsPanel() {
        val open = ViewerChromeState.initial().toggleSheet(ViewerSheet.INFORMATION)

        assertEquals(ViewerSheet.INFORMATION, open.sheet)
        assertEquals(null, open.toggleSheet(ViewerSheet.INFORMATION).sheet)
    }

    @Test fun navigationKeepsFilmstripAndPagerIndexBounded() {
        val navigation = ViewerNavigationState(currentIndex = 2, itemCount = 3)

        assertEquals(2, navigation.select(99).currentIndex)
        assertEquals(ViewerNavigationState(currentIndex = 1, itemCount = 2), navigation.removeCurrent())
    }

    @Test fun seekCommitsOnceOnlyAfterDragFinishes() {
        val dragging = ViewerSeekState.initial()
            .synchronize(positionMs = 2_000L, durationMs = 10_000L)
            .dragTo(7_500f)

        val (committed, target) = requireNotNull(dragging.commitDrag())

        assertEquals(7_500L, target)
        assertEquals(7_500L, committed.positionMs)
        assertEquals(null, committed.previewPositionMs)
        assertEquals(null, committed.commitDrag())
    }

    @Test fun cancelledSeekRestoresTheLastPlayerPosition() {
        val cancelled = ViewerSeekState.initial()
            .synchronize(positionMs = 2_000L, durationMs = 10_000L)
            .dragTo(7_500f)
            .cancelDrag()

        assertEquals(2_000f, cancelled.displayedPositionMs)
    }

    @Test fun openInformationPanelSurvivesRotationAndAdaptsItsSide() {
        val open = ViewerChromeState.initial().openSheet(ViewerSheet.INFORMATION)
        val restored = requireNotNull(restoreViewerChromeState(open.restorationValues()))

        assertEquals(ViewerSheet.INFORMATION, restored.sheet)
        assertEquals(ViewerInformationPresentation.BOTTOM, viewerInformationPresentation(landscape = false))
        assertEquals(ViewerInformationPresentation.RIGHT, viewerInformationPresentation(landscape = true))
        assertEquals(false, restored.interactionActive)
    }
}
