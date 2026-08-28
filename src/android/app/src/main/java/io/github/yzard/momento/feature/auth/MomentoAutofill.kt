package io.github.yzard.momento.feature.auth

import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.ui.ExperimentalComposeUiApi
import androidx.compose.ui.Modifier
import androidx.compose.ui.autofill.Autofill
import androidx.compose.ui.autofill.AutofillNode
import androidx.compose.ui.autofill.AutofillType
import androidx.compose.ui.focus.onFocusChanged
import androidx.compose.ui.geometry.Rect
import androidx.compose.ui.layout.boundsInWindow
import androidx.compose.ui.layout.onGloballyPositioned
import androidx.compose.ui.platform.LocalAutofillTree

enum class PasswordAutofillRole { EXISTING, NEW }

@Suppress("DEPRECATION")
@OptIn(ExperimentalComposeUiApi::class)
fun passwordAutofillTypes(role: PasswordAutofillRole): List<AutofillType> = when (role) {
    PasswordAutofillRole.EXISTING -> listOf(AutofillType.Password)
    PasswordAutofillRole.NEW -> listOf(AutofillType.NewPassword)
}

@OptIn(ExperimentalComposeUiApi::class)
@Suppress("DEPRECATION")
@Composable
fun rememberMomentoAutofillNode(
    autofillTypes: List<AutofillType>,
    onFill: (String) -> Unit,
): AutofillNode {
    val latestOnFill by rememberUpdatedState(onFill)
    val autofillTree = LocalAutofillTree.current
    val node = remember(autofillTypes) {
        AutofillNode(
            autofillTypes = autofillTypes,
            onFill = { value -> latestOnFill(value) },
        )
    }
    DisposableEffect(autofillTree, node) {
        autofillTree += node
        onDispose { autofillTree.children.remove(node.id) }
    }
    return node
}

@OptIn(ExperimentalComposeUiApi::class)
@Suppress("DEPRECATION")
fun Modifier.momentoAutofill(
    autofillNode: AutofillNode,
    autofill: Autofill?,
): Modifier = this
    .onGloballyPositioned { coordinates ->
        autofillNode.boundingBox = coordinates.boundsInWindow().takeUnless { it == Rect.Zero }
    }
    .onFocusChanged { focusState ->
        if (focusState.isFocused) {
            autofill?.requestAutofillForNode(autofillNode)
        } else {
            autofill?.cancelAutofillForNode(autofillNode)
        }
    }
