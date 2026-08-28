package io.github.yzard.momento.feature.media

import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.BoxScope
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.Alignment
import androidx.compose.ui.unit.dp
import io.github.yzard.momento.app.designsystem.MomentoPageScaffold
import io.github.yzard.momento.core.data.AuthenticatedMediaRepository
import io.github.yzard.momento.core.model.Media

@Composable
fun MomentoCollectionDetail(
    title: String,
    subtitle: String,
    backContentDescription: String,
    repository: AuthenticatedMediaRepository,
    pageState: PageState<List<Media>>,
    selectedMediaIds: Set<Long>,
    reserveBottomControls: Boolean,
    bottomContent: (@Composable BoxScope.() -> Unit)?,
    footerContent: (@Composable () -> Unit)?,
    contentError: String?,
    loadingLabel: String,
    emptyTitle: String,
    emptyExplanation: String,
    close: () -> Unit,
    retry: () -> Unit,
    select: (Media, List<Media>) -> Unit,
) {
    MomentoPageScaffold(
        title = title,
        subtitle = subtitle,
        backContentDescription = backContentDescription,
        onBack = close,
        trailingContent = null,
        reserveBottomControls = reserveBottomControls,
        edgeToEdgeContent = true,
        bottomContent = bottomContent,
        modifier = Modifier,
    ) { contentPadding ->
        when (pageState) {
            PageState.Loading -> LoadingState(loadingLabel, Modifier)
            is PageState.Failed -> ErrorState(pageState.message, retry, Modifier)
            is PageState.Ready -> {
                val media = pageState.content
                if (media.isEmpty()) {
                    EmptyState(emptyTitle, emptyExplanation, Modifier)
                } else {
                    MediaGrid(
                        media = media,
                        repository = repository,
                        selectedMediaIds = selectedMediaIds,
                        contentPadding = contentPadding,
                        headerContent = null,
                        footerContent = footerContent,
                        modifier = Modifier.fillMaxSize(),
                    ) { mediaItem ->
                        select(mediaItem, media)
                    }
                    if (pageState.refreshing) {
                        LinearProgressIndicator(
                            modifier = Modifier
                                .align(Alignment.TopCenter)
                                .padding(top = contentPadding.calculateTopPadding()),
                        )
                    }
                }
            }
        }
        contentError?.let { message ->
            Text(
                text = message,
                color = MaterialTheme.colorScheme.error,
                style = MaterialTheme.typography.labelLarge,
                modifier = Modifier
                    .align(Alignment.TopCenter)
                    .padding(
                        top = contentPadding.calculateTopPadding() + 8.dp,
                        start = 20.dp,
                        end = 20.dp,
                    ),
            )
        }
    }
}
