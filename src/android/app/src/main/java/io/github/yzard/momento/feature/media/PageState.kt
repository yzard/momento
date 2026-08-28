package io.github.yzard.momento.feature.media

sealed interface PageState<out Content> {
    data object Loading : PageState<Nothing>

    data class Ready<Content>(
        val content: Content,
        val refreshing: Boolean,
    ) : PageState<Content>

    data class Failed(val message: String) : PageState<Nothing>
}

fun <Content> PageState<Content>.beginRefresh(): PageState<Content> = when (this) {
    is PageState.Ready -> copy(refreshing = true)
    PageState.Loading,
    is PageState.Failed,
    -> PageState.Loading
}

fun <Content> Content.asReadyPage(): PageState<Content> = PageState.Ready(
    content = this,
    refreshing = false,
)

fun <Content> PageState<Content>.failRefresh(message: String): PageState<Content> = when (this) {
    is PageState.Ready -> copy(refreshing = false)
    PageState.Loading,
    is PageState.Failed,
    -> PageState.Failed(message)
}
