package io.github.yzard.momento.app.designsystem

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxScope
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.RowScope
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.asPaddingValues
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.navigationBars
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.statusBars
import androidx.compose.foundation.layout.windowInsetsPadding
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp

data class MomentoPageLayout(
    val horizontalPadding: Int,
    val topPadding: Int,
    val bottomPadding: Int,
)

fun momentoPageLayout(
    widthDp: Int,
    statusBarInsetDp: Int,
    navigationBarInsetDp: Int,
    hasBottomControls: Boolean,
): MomentoPageLayout {
    require(widthDp >= 0) { "Page width must not be negative" }
    require(statusBarInsetDp >= 0) { "Status bar inset must not be negative" }
    require(navigationBarInsetDp >= 0) { "Navigation bar inset must not be negative" }

    val horizontalPadding = when {
        widthDp < 600 -> 12
        widthDp < 840 -> 20
        else -> 28
    }
    val bottomControlClearance = if (hasBottomControls) 92 else 20
    return MomentoPageLayout(
        horizontalPadding = horizontalPadding,
        topPadding = statusBarInsetDp + 80,
        bottomPadding = navigationBarInsetDp + bottomControlClearance,
    )
}

@Composable
fun MomentoPageScaffold(
    title: String,
    subtitle: String?,
    backContentDescription: String?,
    onBack: (() -> Unit)?,
    trailingContent: (@Composable RowScope.() -> Unit)?,
    reserveBottomControls: Boolean,
    bottomContent: (@Composable BoxScope.() -> Unit)?,
    modifier: Modifier,
    content: @Composable BoxScope.(PaddingValues) -> Unit,
) {
    require((backContentDescription == null) == (onBack == null)) {
        "Back description and action must either both be present or both be absent"
    }

    val statusBarPadding = WindowInsets.statusBars.asPaddingValues().calculateTopPadding()
    val navigationBarPadding = WindowInsets.navigationBars.asPaddingValues().calculateBottomPadding()
    Box(
        modifier = modifier
            .fillMaxSize()
            .background(MaterialTheme.colorScheme.background),
    ) {
        content(
            PaddingValues(
                start = adaptivePageHorizontalPadding(),
                top = statusBarPadding + 80.dp,
                end = adaptivePageHorizontalPadding(),
                bottom = navigationBarPadding + if (reserveBottomControls || bottomContent != null) 92.dp else 20.dp,
            ),
        )
        MomentoPageHeader(
            title = title,
            subtitle = subtitle,
            modifier = Modifier
                .align(Alignment.TopStart)
                .windowInsetsPadding(WindowInsets.statusBars),
            leadingContent = onBack?.let { backAction ->
                {
                    IconButton(onClick = backAction) {
                        Icon(
                            imageVector = Icons.AutoMirrored.Filled.ArrowBack,
                            contentDescription = requireNotNull(backContentDescription),
                        )
                    }
                }
            },
            trailingContent = trailingContent,
        )
        if (bottomContent != null) {
            Box(
                modifier = Modifier
                    .align(Alignment.BottomCenter)
                    .windowInsetsPadding(WindowInsets.navigationBars)
                    .padding(bottom = 12.dp),
                content = bottomContent,
            )
        }
    }
}

@Composable
private fun adaptivePageHorizontalPadding(): Dp {
    val screenWidthDp = androidx.compose.ui.platform.LocalConfiguration.current.screenWidthDp
    return momentoPageLayout(
        widthDp = screenWidthDp,
        statusBarInsetDp = 0,
        navigationBarInsetDp = 0,
        hasBottomControls = false,
    ).horizontalPadding.dp
}
