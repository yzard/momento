package io.github.yzard.momento.feature.timeline

import androidx.compose.material3.MaterialTheme
import androidx.compose.runtime.key
import androidx.compose.runtime.mutableStateOf
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onNodeWithText
import org.junit.Rule
import org.junit.Test
import java.time.LocalDate

class TimelinePeriodPickerInstrumentedTest {
    @get:Rule
    val composeRule = createComposeRule()

    @Test
    fun periodControlsOpenTheirMatchingSelector() {
        val activePeriod = mutableStateOf(TimelinePeriod.DAY)
        composeRule.setContent {
            MaterialTheme {
                key(activePeriod.value) {
                    TimelinePeriodPicker(
                        period = activePeriod.value,
                        initialDate = LocalDate.of(2026, 8, 27),
                        dismiss = {},
                        select = {},
                    )
                }
            }
        }

        composeRule.onNodeWithText("Select date").fetchSemanticsNode()
        composeRule.runOnIdle { activePeriod.value = TimelinePeriod.WEEK }
        composeRule.onNodeWithText("Select week").fetchSemanticsNode()
        composeRule.runOnIdle { activePeriod.value = TimelinePeriod.MONTH }
        composeRule.onNodeWithText("Select month").fetchSemanticsNode()
        composeRule.runOnIdle { activePeriod.value = TimelinePeriod.YEAR }
        composeRule.onNodeWithText("Select year").fetchSemanticsNode()
    }
}
