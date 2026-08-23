package io.github.yzard.momento.feature.media

import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier

@Composable fun LoadingState() = Box(Modifier.fillMaxSize(), contentAlignment = Alignment.Center) { CircularProgressIndicator() }
@Composable fun EmptyState(message: String) = Box(Modifier.fillMaxSize(), contentAlignment = Alignment.Center) { Text(message) }
@Composable fun ErrorState(message: String, retry: () -> Unit) = Box(Modifier.fillMaxSize(), contentAlignment = Alignment.Center) { TextButton(retry) { Text("$message. Retry") } }
@Composable fun ConfirmDialog(title: String, message: String, accept: () -> Unit, dismiss: () -> Unit) = AlertDialog(onDismissRequest = dismiss, title = { Text(title) }, text = { Text(message) }, confirmButton = { TextButton(accept) { Text("Confirm") } }, dismissButton = { TextButton(dismiss) { Text("Cancel") } })
