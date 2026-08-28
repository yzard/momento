package io.github.yzard.momento.feature.admin

import androidx.compose.foundation.background
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.CleaningServices
import androidx.compose.material.icons.filled.PlayArrow
import androidx.compose.material.icons.filled.Save
import androidx.compose.material.icons.filled.Stop
import androidx.compose.material3.Button
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.Icon
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import io.github.yzard.momento.core.data.AdministrationRepository
import io.github.yzard.momento.core.model.AiFeatureSchedule
import io.github.yzard.momento.core.model.AiJobCounts
import io.github.yzard.momento.core.model.AiStatusResponse
import kotlinx.coroutines.launch
import kotlinx.serialization.SerializationException
import retrofit2.HttpException
import java.io.IOException

@Composable
internal fun AiAdministration(
    repository: AdministrationRepository,
    status: AiStatusResponse?,
    error: String?,
    refresh: () -> Unit,
) {
    val controls = listOf<Pair<AdminAiFeature?, String>>(null to "All AI jobs") +
        AdminAiFeature.entries.map { it to it.label }
    var busyControls by remember { mutableStateOf<Set<String>>(emptySet()) }
    var actionError by remember { mutableStateOf<String?>(null) }
    var pendingAction by remember { mutableStateOf<PendingAdminAction?>(null) }
    val scope = rememberCoroutineScope()

    fun taskState(feature: AdminAiFeature?): String? {
        if (feature == null) {
            return if (
                AdminAiFeature.entries.any { currentFeature ->
                    isActiveAiState(taskState(currentFeature))
                }
            ) "running" else "idle"
        }
        if (feature == AdminAiFeature.DEDUPLICATE) return status?.deduplicate?.status
        return status?.tasks?.firstOrNull { it.task == feature.identifier }?.state
    }

    fun runAction(
        controlKey: String,
        actionLabel: String,
        action: suspend () -> Unit,
    ) {
        if (controlKey in busyControls) return
        scope.launch {
            busyControls += controlKey
            try {
                action()
                actionError = null
                refresh()
            } catch (_: IOException) {
                actionError = "$actionLabel failed"
            } catch (_: HttpException) {
                actionError = "$actionLabel failed"
            } catch (_: SerializationException) {
                actionError = "$actionLabel failed"
            } finally {
                busyControls -= controlKey
            }
        }
    }

    fun actionFor(feature: AdminAiFeature?, actionName: String): suspend () -> Unit = {
        when (actionName) {
            "start" -> if (feature == null) repository.startAi() else repository.startAiFeature(feature.identifier)
            "cancel" -> if (feature == null) repository.cancelAi() else repository.cancelAiFeature(feature.identifier)
            "clean" -> if (feature == null) repository.cleanAi() else repository.cleanAiFeature(feature.identifier)
            else -> error("Unknown AI action $actionName")
        }
        Unit
    }

    LazyColumn(
        modifier = Modifier.fillMaxSize(),
        contentPadding = PaddingValues(16.dp, 16.dp, 16.dp, 104.dp),
        verticalArrangement = Arrangement.spacedBy(12.dp),
    ) {
        item {
            AdminPanel("AI processing", "Each task runs independently through the durable server queue.") {
                status?.let { currentStatus ->
                    Text("${currentStatus.faceGroups} face groups · ${currentStatus.deduplicate.clustersCreated} duplicate groups")
                }
                error?.let { AdminError(it) }
                actionError?.let { AdminError(it) }
            }
        }
        item {
            AdminPanel("AI work status", "Queued, submitting, submitted, failed, and completed jobs by feature.") {
                AiStatusTable(status)
            }
        }
        items(controls, key = { it.second }) { (feature, label) ->
            val state = taskState(feature)
            val running = isActiveAiState(state)
            val controlKey = feature?.identifier ?: "all"
            Card(colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surfaceContainerLow)) {
                Column(Modifier.fillMaxWidth().padding(16.dp)) {
                    Text(label, style = MaterialTheme.typography.titleMedium, fontWeight = FontWeight.SemiBold)
                    Text(state ?: "Not loaded", color = MaterialTheme.colorScheme.onSurfaceVariant)
                    if (feature != null && feature != AdminAiFeature.DEDUPLICATE) {
                        status?.tasks?.firstOrNull { it.task == feature.identifier }?.let { task ->
                            Text(aiStatusSummary(task), style = MaterialTheme.typography.bodySmall)
                            task.errors.forEach { AdminError(it) }
                        }
                    }
                    Row(
                        modifier = Modifier.fillMaxWidth().padding(top = 12.dp),
                        horizontalArrangement = Arrangement.spacedBy(8.dp, Alignment.End),
                    ) {
                        val primaryControlKey = "$controlKey-primary"
                        val cleanControlKey = "$controlKey-clean"
                        val primaryActionLabel = if (running) "Cancel $label" else "Start $label"
                        Button(
                            onClick = {
                                if (running) {
                                    pendingAction = PendingAdminAction(
                                        title = "Cancel $label?",
                                        description = "Queued and active work for this control will be cancelled.",
                                        confirmLabel = "Cancel jobs",
                                        execute = {
                                            runAction(primaryControlKey, "Cancel $label", actionFor(feature, "cancel"))
                                        },
                                    )
                                } else {
                                    runAction(primaryControlKey, "Start $label", actionFor(feature, "start"))
                                }
                            },
                            enabled = primaryControlKey !in busyControls,
                            modifier = Modifier
                                .size(48.dp)
                                .semantics { contentDescription = primaryActionLabel },
                            contentPadding = PaddingValues(0.dp),
                        ) {
                            if (primaryControlKey in busyControls) {
                                CircularProgressIndicator(
                                    modifier = Modifier.size(20.dp),
                                    strokeWidth = 2.dp,
                                    color = MaterialTheme.colorScheme.onPrimary,
                                )
                            } else {
                                Icon(
                                    if (running) Icons.Default.Stop else Icons.Default.PlayArrow,
                                    contentDescription = null,
                                )
                            }
                        }
                        OutlinedButton(
                            onClick = {
                                pendingAction = PendingAdminAction(
                                    title = "Clean $label data?",
                                    description = "Stored results and eligible job state for this control will be removed.",
                                    confirmLabel = "Clean data",
                                    execute = {
                                        runAction(cleanControlKey, "Clean $label", actionFor(feature, "clean"))
                                    },
                                )
                            },
                            enabled = !running && cleanControlKey !in busyControls,
                            modifier = Modifier
                                .size(48.dp)
                                .semantics { contentDescription = "Clean $label data" },
                            contentPadding = PaddingValues(0.dp),
                        ) {
                            if (cleanControlKey in busyControls) {
                                CircularProgressIndicator(
                                    modifier = Modifier.size(20.dp),
                                    strokeWidth = 2.dp,
                                )
                            } else {
                                Icon(Icons.Default.CleaningServices, contentDescription = null)
                            }
                        }
                    }
                    if (feature != null) {
                        status?.schedules?.firstOrNull { it.feature == feature.identifier }?.let { schedule ->
                            AiScheduleEditor(
                                label = label,
                                schedule = schedule,
                                busy = "${controlKey}-schedule" in busyControls,
                                save = { cronExpression ->
                                    runAction("${controlKey}-schedule", "Save $label schedule") {
                                        repository.updateAiSchedule(feature.identifier, cronExpression)
                                        Unit
                                    }
                                },
                            )
                        }
                    }
                }
            }
        }
    }
    pendingAction?.let { action ->
        ConfirmationDialog(
            action = action,
            confirm = {
                pendingAction = null
                scope.launch { action.execute() }
            },
            dismiss = { pendingAction = null },
        )
    }
}

@Composable
private fun AiStatusTable(status: AiStatusResponse?) {
    BoxWithConstraints(Modifier.fillMaxWidth()) {
        if (maxWidth < 720.dp) {
            Column(verticalArrangement = Arrangement.spacedBy(10.dp)) {
                AdminAiFeature.entries.forEach { feature ->
                    val jobs = aiJobCounts(status, feature)
                    Column(
                        Modifier
                            .fillMaxWidth()
                            .background(MaterialTheme.colorScheme.surfaceContainer, RoundedCornerShape(12.dp))
                            .padding(12.dp),
                    ) {
                        Text(feature.label, fontWeight = FontWeight.SemiBold)
                        Text(
                            "${jobs?.queued ?: 0} queued · ${jobs?.submitting ?: 0} submitting · ${jobs?.submitted ?: 0} submitted",
                            style = MaterialTheme.typography.bodySmall,
                        )
                        Text(
                            "${jobs?.failed ?: 0} failed · ${jobs?.completed ?: 0} completed",
                            style = MaterialTheme.typography.bodySmall,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                        )
                    }
                }
            }
        } else {
            Column(
                Modifier.fillMaxWidth().horizontalScroll(rememberScrollState()).width(720.dp),
            ) {
                AiStatusRow("Feature", listOf("Queued", "Submitting", "Submitted", "Failed", "Completed"), true)
                HorizontalDivider(Modifier.padding(vertical = 6.dp))
                AdminAiFeature.entries.forEach { feature ->
                    val jobs = aiJobCounts(status, feature)
                    AiStatusRow(
                        feature.label,
                        listOf(
                            jobs?.queued?.toString() ?: "0",
                            jobs?.submitting?.toString() ?: "0",
                            jobs?.submitted?.toString() ?: "0",
                            jobs?.failed?.toString() ?: "0",
                            jobs?.completed?.toString() ?: "0",
                        ),
                        false,
                    )
                }
            }
        }
    }
}

@Composable
private fun AiStatusRow(label: String, values: List<String>, header: Boolean) {
    Row(Modifier.fillMaxWidth().padding(vertical = 6.dp)) {
        Text(
            label,
            modifier = Modifier.width(180.dp),
            fontWeight = if (header) FontWeight.Bold else FontWeight.SemiBold,
            style = MaterialTheme.typography.bodySmall,
        )
        values.forEach { value ->
            Text(
                value,
                modifier = Modifier.width(108.dp),
                textAlign = TextAlign.End,
                fontWeight = if (header) FontWeight.Bold else FontWeight.Normal,
                style = MaterialTheme.typography.bodySmall,
            )
        }
    }
}

@Composable
private fun AiScheduleEditor(
    label: String,
    schedule: AiFeatureSchedule,
    busy: Boolean,
    save: (String) -> Unit,
) {
    var cronFields by remember(schedule.cronExpression) {
        mutableStateOf(splitCronExpression(schedule.cronExpression))
    }
    val cronExpression = joinCronFields(cronFields)
    val fieldsValid = validCronFields(cronFields)
    val storedExpression = joinCronFields(splitCronExpression(schedule.cronExpression))
    Column(Modifier.fillMaxWidth().padding(top = 12.dp)) {
        Text("Schedule · five-field cron · system timezone", style = MaterialTheme.typography.labelSmall)
        BoxWithConstraints(Modifier.fillMaxWidth().padding(top = 6.dp)) {
            val fieldsPerRow = cronFieldsPerRow(maxWidth.value.toInt())
            Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
                cronFieldLabels.indices.chunked(fieldsPerRow).forEach { rowIndices ->
                    Row(
                        modifier = Modifier.fillMaxWidth(),
                        horizontalArrangement = Arrangement.spacedBy(8.dp),
                    ) {
                        rowIndices.forEach { index ->
                            Column(Modifier.weight(1f)) {
                                Text(
                                    cronFieldLabels[index],
                                    style = MaterialTheme.typography.labelSmall,
                                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                                    maxLines = 1,
                                )
                                OutlinedTextField(
                                    value = cronFields[index],
                                    onValueChange = { fieldValue ->
                                        cronFields = cronFields.toMutableList().also { fields ->
                                            fields[index] = fieldValue
                                        }
                                    },
                                    singleLine = true,
                                    isError = cronFields[index].trim().isEmpty() ||
                                        cronFields[index].trim().contains(Regex("\\s")),
                                    modifier = Modifier.fillMaxWidth(),
                                )
                            }
                        }
                        repeat(fieldsPerRow - rowIndices.size) { Box(Modifier.weight(1f)) }
                    }
                }
                OutlinedButton(
                    onClick = { save(cronExpression) },
                    enabled = !busy && fieldsValid && cronExpression != storedExpression,
                    modifier = Modifier.align(Alignment.End).semantics {
                        contentDescription = "Save $label cron schedule"
                    },
                ) {
                    Icon(Icons.Default.Save, contentDescription = null)
                    Text("Save schedule", Modifier.padding(start = 8.dp))
                }
            }
        }
        if (!fieldsValid) {
            Text(
                "Every cron field must contain one value without spaces.",
                color = MaterialTheme.colorScheme.error,
                style = MaterialTheme.typography.bodySmall,
                modifier = Modifier.padding(top = 6.dp),
            )
        }
    }
}
