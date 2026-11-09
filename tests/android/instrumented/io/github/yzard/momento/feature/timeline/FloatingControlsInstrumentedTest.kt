package io.github.yzard.momento.feature.timeline

import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.test.*
import androidx.compose.ui.test.junit4.createComposeRule
import io.github.yzard.momento.app.designsystem.MomentoTheme
import io.github.yzard.momento.app.designsystem.momentoFloatingControlHeight
import io.github.yzard.momento.app.navigation.Destination
import io.github.yzard.momento.app.navigation.ShellOverlay
import io.github.yzard.momento.core.data.ThemePreference
import org.junit.Rule
import org.junit.Test

class FloatingControlsInstrumentedTest {
    @get:Rule val rule = createComposeRule()

    @Test fun allTimelineControlsAndExpandedSearchShareOneHeight() {
        rule.setContent {
            MomentoTheme(ThemePreference.LIGHT) {
                Box(Modifier.fillMaxSize()) {
                    FloatingTimelineHeader("2026-11-07", {}, Modifier.align(Alignment.TopCenter))
                    TimelineSelectionControl(false, {}, {}, Modifier.align(Alignment.TopEnd))
                    ShellOverlay(Destination.TIMELINE, TimelinePeriod.DAY, "", {}, {}, {})
                }
            }
        }
        for (tag in listOf("timeline-date-control", "timeline-selection-control", "timeline-menu-button", "timeline-period-dock", "timeline-search-button")) {
            rule.onNodeWithTag(tag).assertHeightIsEqualTo(momentoFloatingControlHeight)
        }
        rule.onNodeWithContentDescription("Open search").performClick()
        rule.onNodeWithTag("timeline-search-bar").assertHeightIsEqualTo(momentoFloatingControlHeight)
        rule.onNodeWithTag("timeline-menu-button").assertHeightIsEqualTo(momentoFloatingControlHeight)
        rule.onNodeWithTag("timeline-search-button").assertHeightIsEqualTo(momentoFloatingControlHeight)
    }
}
