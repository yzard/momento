package io.github.yzard.momento.feature.timeline

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.lazy.grid.GridCells
import androidx.compose.foundation.lazy.grid.LazyVerticalGrid
import androidx.compose.foundation.lazy.grid.items
import androidx.compose.foundation.lazy.grid.rememberLazyGridState
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.automirrored.filled.ArrowForward
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.DatePicker
import androidx.compose.material3.DatePickerDialog
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.FilterChip
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.rememberDatePickerState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.key
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import java.time.DateTimeException
import java.time.Instant
import java.time.LocalDate
import java.time.Month
import java.time.ZoneOffset
import java.time.format.DateTimeFormatter
import java.time.format.DateTimeParseException
import java.time.format.TextStyle
import java.time.temporal.ChronoField
import java.time.temporal.IsoFields
import java.time.temporal.TemporalAdjusters
import java.util.Locale

private const val DEFAULT_FIRST_PICKER_YEAR = 1900
private const val DEFAULT_LAST_PICKER_YEAR = 2100
private val weekLabelFormatter = DateTimeFormatter.ofPattern("MMM d", Locale.US)

fun timelinePeriodAnchorDate(period: TimelinePeriod, selectedDate: LocalDate): String {
    val periodEnd = when (period) {
        TimelinePeriod.DAY -> selectedDate
        TimelinePeriod.WEEK -> selectedDate
            .with(TemporalAdjusters.previousOrSame(java.time.DayOfWeek.MONDAY))
            .plusDays(6)
        TimelinePeriod.MONTH -> selectedDate.with(TemporalAdjusters.lastDayOfMonth())
        TimelinePeriod.YEAR -> LocalDate.of(selectedDate.year, 12, 31)
    }
    return periodEnd.plusDays(1).atStartOfDay(ZoneOffset.UTC).toInstant().minusMillis(1).toString()
}

fun timelinePeriodInitialDate(period: TimelinePeriod, label: String, fallback: LocalDate): LocalDate =
    when (period) {
        TimelinePeriod.DAY -> parseTimelineDay(label) ?: fallback
        TimelinePeriod.WEEK -> parseTimelineWeek(label) ?: fallback
        TimelinePeriod.MONTH -> parseTimelineMonth(label) ?: fallback
        TimelinePeriod.YEAR -> parseTimelineYear(label) ?: fallback
    }

fun timelinePeriodPickerTitle(period: TimelinePeriod): String = when (period) {
    TimelinePeriod.DAY -> "Select date"
    TimelinePeriod.WEEK -> "Select week"
    TimelinePeriod.MONTH -> "Select month"
    TimelinePeriod.YEAR -> "Select year"
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
internal fun TimelinePeriodPicker(
    period: TimelinePeriod,
    initialDate: LocalDate,
    dismiss: () -> Unit,
    select: (LocalDate) -> Unit,
) {
    when (period) {
        TimelinePeriod.DAY -> DayPickerDialog(initialDate, dismiss, select)
        TimelinePeriod.WEEK -> WeekPickerDialog(initialDate, dismiss, select)
        TimelinePeriod.MONTH -> MonthPickerDialog(initialDate, dismiss, select)
        TimelinePeriod.YEAR -> YearPickerDialog(initialDate, dismiss, select)
    }
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun DayPickerDialog(
    initialDate: LocalDate,
    dismiss: () -> Unit,
    select: (LocalDate) -> Unit,
) {
    val initialMillis = initialDate.atStartOfDay(ZoneOffset.UTC).toInstant().toEpochMilli()
    val state = rememberDatePickerState(
        initialSelectedDateMillis = initialMillis,
        yearRange = pickerYearRange(initialDate.year),
    )
    DatePickerDialog(
        onDismissRequest = dismiss,
        confirmButton = {
            TextButton(
                enabled = state.selectedDateMillis != null,
                onClick = {
                    val selectedMillis = state.selectedDateMillis ?: return@TextButton
                    select(Instant.ofEpochMilli(selectedMillis).atZone(ZoneOffset.UTC).toLocalDate())
                },
            ) { Text("Jump") }
        },
        dismissButton = { TextButton(onClick = dismiss) { Text("Cancel") } },
    ) {
        DatePicker(state = state, title = { Text(timelinePeriodPickerTitle(TimelinePeriod.DAY)) })
    }
}

@Composable
private fun WeekPickerDialog(
    initialDate: LocalDate,
    dismiss: () -> Unit,
    select: (LocalDate) -> Unit,
) {
    var selectedWeekStart by remember { mutableStateOf(initialDate.startOfIsoWeek()) }
    var displayedYear by remember { mutableIntStateOf(initialDate.get(IsoFields.WEEK_BASED_YEAR)) }
    val yearRange = pickerYearRange(displayedYear)
    val weeks = remember(displayedYear) {
        (1..isoWeeksInYear(displayedYear)).map { week -> isoWeekStart(displayedYear, week) }
    }
    fun showYear(year: Int) {
        val selectedWeek = selectedWeekStart.get(IsoFields.WEEK_OF_WEEK_BASED_YEAR)
        displayedYear = year
        selectedWeekStart = isoWeekStart(year, minOf(selectedWeek, isoWeeksInYear(year)))
    }

    PeriodPickerDialog(
        title = timelinePeriodPickerTitle(TimelinePeriod.WEEK),
        dismiss = dismiss,
        confirm = { select(selectedWeekStart) },
    ) {
        PickerYearNavigation(
            year = displayedYear,
            previousEnabled = displayedYear > yearRange.first,
            nextEnabled = displayedYear < yearRange.last,
            previous = { showYear(displayedYear - 1) },
            next = { showYear(displayedYear + 1) },
        )
        key(displayedYear) {
            PeriodChoiceGrid(
                choices = weeks,
                columns = 2,
                selected = selectedWeekStart,
                label = { weekStart ->
                    val week = weekStart.get(IsoFields.WEEK_OF_WEEK_BASED_YEAR)
                    "W${week.toString().padStart(2, '0')}  ${weekLabelFormatter.format(weekStart)}–" +
                        weekLabelFormatter.format(weekStart.plusDays(6))
                },
                choose = { selectedWeekStart = it },
            )
        }
    }
}

@Composable
private fun MonthPickerDialog(
    initialDate: LocalDate,
    dismiss: () -> Unit,
    select: (LocalDate) -> Unit,
) {
    var selectedMonth by remember { mutableStateOf(initialDate.withDayOfMonth(1)) }
    var displayedYear by remember { mutableIntStateOf(initialDate.year) }
    val yearRange = pickerYearRange(displayedYear)
    val months = remember(displayedYear) { Month.entries.map { LocalDate.of(displayedYear, it, 1) } }
    fun showYear(year: Int) {
        displayedYear = year
        selectedMonth = LocalDate.of(year, selectedMonth.month, 1)
    }

    PeriodPickerDialog(
        title = timelinePeriodPickerTitle(TimelinePeriod.MONTH),
        dismiss = dismiss,
        confirm = { select(selectedMonth) },
    ) {
        PickerYearNavigation(
            year = displayedYear,
            previousEnabled = displayedYear > yearRange.first,
            nextEnabled = displayedYear < yearRange.last,
            previous = { showYear(displayedYear - 1) },
            next = { showYear(displayedYear + 1) },
        )
        PeriodChoiceGrid(
            choices = months,
            columns = 3,
            selected = selectedMonth,
            label = { it.month.getDisplayName(TextStyle.FULL, Locale.US) },
            choose = { selectedMonth = it },
        )
    }
}

@Composable
private fun YearPickerDialog(
    initialDate: LocalDate,
    dismiss: () -> Unit,
    select: (LocalDate) -> Unit,
) {
    val yearRange = pickerYearRange(initialDate.year)
    var selectedYear by remember { mutableIntStateOf(initialDate.year) }
    val years = remember(yearRange) { yearRange.toList() }
    val initialRow = ((initialDate.year - yearRange.first) / 3).coerceAtLeast(0)
    val gridState = rememberLazyGridState(initialFirstVisibleItemIndex = initialRow * 3)

    PeriodPickerDialog(
        title = timelinePeriodPickerTitle(TimelinePeriod.YEAR),
        dismiss = dismiss,
        confirm = { select(LocalDate.of(selectedYear, 1, 1)) },
    ) {
        LazyVerticalGrid(
            columns = GridCells.Fixed(3),
            state = gridState,
            modifier = Modifier.fillMaxWidth().heightIn(max = 360.dp),
            horizontalArrangement = Arrangement.spacedBy(8.dp),
            verticalArrangement = Arrangement.spacedBy(8.dp),
        ) {
            items(years) { year ->
                PeriodChoice(
                    label = year.toString(),
                    selected = year == selectedYear,
                    choose = { selectedYear = year },
                )
            }
        }
    }
}

@Composable
private fun PeriodPickerDialog(
    title: String,
    dismiss: () -> Unit,
    confirm: () -> Unit,
    content: @Composable () -> Unit,
) {
    AlertDialog(
        onDismissRequest = dismiss,
        title = { Text(title) },
        text = { Column(verticalArrangement = Arrangement.spacedBy(12.dp)) { content() } },
        confirmButton = { TextButton(onClick = confirm) { Text("Jump") } },
        dismissButton = { TextButton(onClick = dismiss) { Text("Cancel") } },
    )
}

@Composable
private fun PickerYearNavigation(
    year: Int,
    previousEnabled: Boolean,
    nextEnabled: Boolean,
    previous: () -> Unit,
    next: () -> Unit,
) {
    Row(
        modifier = Modifier.fillMaxWidth(),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.SpaceBetween,
    ) {
        IconButton(onClick = previous, enabled = previousEnabled) {
            Icon(Icons.AutoMirrored.Filled.ArrowBack, "Previous year")
        }
        Text(year.toString())
        IconButton(onClick = next, enabled = nextEnabled) {
            Icon(Icons.AutoMirrored.Filled.ArrowForward, "Next year")
        }
    }
}

@Composable
private fun PeriodChoiceGrid(
    choices: List<LocalDate>,
    columns: Int,
    selected: LocalDate,
    label: (LocalDate) -> String,
    choose: (LocalDate) -> Unit,
) {
    val selectedIndex = choices.indexOf(selected).coerceAtLeast(0)
    val initialItemIndex = selectedIndex / columns * columns
    val gridState = rememberLazyGridState(initialFirstVisibleItemIndex = initialItemIndex)
    val rowCount = (choices.size + columns - 1) / columns
    val gridHeight = minOf(360, rowCount * 56).dp
    LazyVerticalGrid(
        columns = GridCells.Fixed(columns),
        state = gridState,
        modifier = Modifier.fillMaxWidth().heightIn(max = gridHeight),
        horizontalArrangement = Arrangement.spacedBy(8.dp),
        verticalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        items(choices) { choice ->
            PeriodChoice(
                label = label(choice),
                selected = choice == selected,
                choose = { choose(choice) },
            )
        }
    }
}

@Composable
private fun PeriodChoice(label: String, selected: Boolean, choose: () -> Unit) {
    FilterChip(
        selected = selected,
        onClick = choose,
        label = { Box(Modifier.fillMaxWidth(), contentAlignment = Alignment.Center) { Text(label) } },
        modifier = Modifier.fillMaxWidth(),
    )
}

private fun pickerYearRange(selectedYear: Int): IntRange =
    minOf(DEFAULT_FIRST_PICKER_YEAR, selectedYear)..maxOf(DEFAULT_LAST_PICKER_YEAR, selectedYear)

private fun LocalDate.startOfIsoWeek(): LocalDate =
    with(ChronoField.DAY_OF_WEEK, java.time.DayOfWeek.MONDAY.value.toLong())

private fun isoWeeksInYear(year: Int): Int = LocalDate.of(year, 12, 28).get(IsoFields.WEEK_OF_WEEK_BASED_YEAR)

private fun isoWeekStart(year: Int, week: Int): LocalDate = LocalDate.of(year, 1, 4)
    .with(IsoFields.WEEK_OF_WEEK_BASED_YEAR, week.toLong())
    .with(ChronoField.DAY_OF_WEEK, java.time.DayOfWeek.MONDAY.value.toLong())

private fun parseTimelineDay(label: String): LocalDate? = try {
    LocalDate.parse(label)
} catch (_: DateTimeParseException) {
    null
}

private fun parseTimelineWeek(label: String): LocalDate? {
    val match = Regex("^(\\d+)-W(\\d{2})$").matchEntire(label) ?: return null
    val year = match.groupValues[1].toIntOrNull() ?: return null
    val week = match.groupValues[2].toIntOrNull() ?: return null
    if (year !in java.time.Year.MIN_VALUE..java.time.Year.MAX_VALUE) return null
    if (week !in 1..isoWeeksInYear(year)) return null
    return isoWeekStart(year, week)
}

private fun parseTimelineMonth(label: String): LocalDate? {
    val match = Regex("^(\\d+)-(\\d{2})$").matchEntire(label) ?: return null
    val year = match.groupValues[1].toIntOrNull() ?: return null
    val month = match.groupValues[2].toIntOrNull() ?: return null
    if (year !in java.time.Year.MIN_VALUE..java.time.Year.MAX_VALUE || month !in 1..12) return null
    return LocalDate.of(year, month, 1)
}

private fun parseTimelineYear(label: String): LocalDate? {
    val year = label.toIntOrNull() ?: return null
    return try {
        LocalDate.of(year, 1, 1)
    } catch (_: DateTimeException) {
        null
    }
}
