package io.github.yzard.momento.feature.media

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.widthIn
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.outlined.Collections
import androidx.compose.material.icons.outlined.ErrorOutline
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp

@Composable
fun LoadingState() = StateLayout {
    CircularProgressIndicator()
    Text("Loading", style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
}

@Composable
fun EmptyState(message: String) = StateLayout {
    Icon(Icons.Outlined.Collections, contentDescription = null, tint = MaterialTheme.colorScheme.onSurfaceVariant)
    Text(message, style = MaterialTheme.typography.titleMedium, textAlign = TextAlign.Center)
    Text(
        "New memories will appear here when they are available.",
        style = MaterialTheme.typography.bodyMedium,
        color = MaterialTheme.colorScheme.onSurfaceVariant,
        textAlign = TextAlign.Center,
    )
}

@Composable
fun ErrorState(message: String, retry: () -> Unit) = StateLayout {
    Icon(Icons.Outlined.ErrorOutline, contentDescription = null, tint = MaterialTheme.colorScheme.error)
    Text(message, style = MaterialTheme.typography.titleMedium, textAlign = TextAlign.Center)
    TextButton(onClick = retry) { Text("Try again") }
}

@Composable
private fun StateLayout(content: @Composable () -> Unit) {
    Column(
        modifier = Modifier.fillMaxSize().padding(32.dp).widthIn(max = 440.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.spacedBy(12.dp, Alignment.CenterVertically),
    ) {
        content()
    }
}

@Composable fun ConfirmDialog(title: String, message: String, accept: () -> Unit, dismiss: () -> Unit) = AlertDialog(onDismissRequest = dismiss, title = { Text(title) }, text = { Text(message) }, confirmButton = { TextButton(accept) { Text("Confirm") } }, dismissButton = { TextButton(dismiss) { Text("Cancel") } })
