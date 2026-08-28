package io.github.yzard.momento.feature.timeline

import org.junit.Assert.assertEquals
import org.junit.Test
import java.time.LocalDate

class TimelinePeriodPickerTest {
    private val fallback = LocalDate.of(2026, 8, 27)

    @Test
    fun eachTimelinePeriodUsesItsOwnSelector() {
        assertEquals("Select date", timelinePeriodPickerTitle(TimelinePeriod.DAY))
        assertEquals("Select week", timelinePeriodPickerTitle(TimelinePeriod.WEEK))
        assertEquals("Select month", timelinePeriodPickerTitle(TimelinePeriod.MONTH))
        assertEquals("Select year", timelinePeriodPickerTitle(TimelinePeriod.YEAR))
    }

    @Test
    fun parsesVisiblePeriodLabelsIntoPickerSelections() {
        assertEquals(LocalDate.of(2026, 8, 27), timelinePeriodInitialDate(TimelinePeriod.DAY, "2026-08-27", fallback))
        assertEquals(LocalDate.of(2026, 8, 24), timelinePeriodInitialDate(TimelinePeriod.WEEK, "2026-W35", fallback))
        assertEquals(LocalDate.of(2026, 8, 1), timelinePeriodInitialDate(TimelinePeriod.MONTH, "2026-08", fallback))
        assertEquals(LocalDate.of(2026, 1, 1), timelinePeriodInitialDate(TimelinePeriod.YEAR, "2026", fallback))
    }

    @Test
    fun invalidOrUnknownPeriodLabelsUseTheCurrentDate() {
        TimelinePeriod.entries.forEach { period ->
            assertEquals(fallback, timelinePeriodInitialDate(period, "Unknown", fallback))
        }
        assertEquals(fallback, timelinePeriodInitialDate(TimelinePeriod.WEEK, "2026-W54", fallback))
        assertEquals(fallback, timelinePeriodInitialDate(TimelinePeriod.MONTH, "2026-13", fallback))
    }

    @Test
    fun selectedPeriodsAnchorAtTheirFinalMillisecond() {
        val selectedDate = LocalDate.of(2026, 8, 27)

        assertEquals("2026-08-27T23:59:59.999Z", timelinePeriodAnchorDate(TimelinePeriod.DAY, selectedDate))
        assertEquals("2026-08-30T23:59:59.999Z", timelinePeriodAnchorDate(TimelinePeriod.WEEK, selectedDate))
        assertEquals("2026-08-31T23:59:59.999Z", timelinePeriodAnchorDate(TimelinePeriod.MONTH, selectedDate))
        assertEquals("2026-12-31T23:59:59.999Z", timelinePeriodAnchorDate(TimelinePeriod.YEAR, selectedDate))
    }

    @Test
    fun isoWeekSelectionHandlesYearBoundaries() {
        assertEquals(
            LocalDate.of(2025, 12, 29),
            timelinePeriodInitialDate(TimelinePeriod.WEEK, "2026-W01", fallback),
        )
        assertEquals(
            "2026-01-04T23:59:59.999Z",
            timelinePeriodAnchorDate(TimelinePeriod.WEEK, LocalDate.of(2025, 12, 29)),
        )
    }
}
