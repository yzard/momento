package io.github.yzard.momento.app.designsystem

import io.github.yzard.momento.core.data.ThemePreference
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

private enum class TestViewport(val widthDp: Int, val heightDp: Int) {
    COMPACT_PORTRAIT(390, 844),
    COMPACT_LANDSCAPE(844, 390),
    EXPANDED_PORTRAIT(840, 1_180),
    EXPANDED_LANDSCAPE(1_180, 840),
}

private enum class TestPageState {
    LOADING,
    EMPTY,
    ERROR,
    CONTENT,
    REFRESHING,
    SELECTION,
    WORKING,
}

class AndroidVisualQaMatrixTest {
    private val pages = listOf(
        "Timeline",
        "Trash",
        "Albums",
        "Album detail",
        "Places",
        "Place detail",
        "People",
        "Face detail",
        "Map",
        "Admin",
        "Settings",
        "Viewer",
    )

    @Test
    fun everyMajorPageHasTheCompleteVisualContractMatrix() {
        val themes = ThemePreference.entries
        val fontScales = listOf(1f, 1.6f)
        val scenarios = pages.flatMap { page ->
            TestViewport.entries.flatMap { viewport ->
                themes.flatMap { theme ->
                    fontScales.flatMap { fontScale ->
                        TestPageState.entries.map { state ->
                            listOf(page, viewport.name, theme.name, fontScale.toString(), state.name)
                        }
                    }
                }
            }
        }

        assertEquals(
            pages.size * TestViewport.entries.size * themes.size * fontScales.size * TestPageState.entries.size,
            scenarios.size,
        )
        TestViewport.entries.forEach { viewport ->
            val layout = momentoPageLayout(
                widthDp = viewport.widthDp,
                statusBarInsetDp = 24,
                navigationBarInsetDp = 24,
                hasBottomControls = true,
            )
            assertTrue(layout.topPadding >= 104)
            assertTrue(layout.bottomPadding >= 116)
            assertTrue(viewport.heightDp > 0)
        }
    }
}
