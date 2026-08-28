package io.github.yzard.momento.core.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.size
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.outlined.ImageNotSupported
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import coil.compose.SubcomposeAsyncImage
import coil.compose.SubcomposeAsyncImageContent
import coil.request.ImageRequest
import io.github.yzard.momento.core.data.AuthenticatedMediaRepository

@Composable
fun MomentoAsyncImage(
    model: Any?,
    repository: AuthenticatedMediaRepository,
    contentDescription: String?,
    contentScale: ContentScale,
    modifier: Modifier,
) {
    val context = LocalContext.current
    val imageSource = remember(repository) { AuthenticatedImageSource(repository) }
    val accessibleModifier = if (contentDescription == null) {
        modifier
    } else {
        modifier.semantics { this.contentDescription = contentDescription }
    }
    Box(accessibleModifier) {
        SubcomposeAsyncImage(
            model = imageSource.request(
                context,
                AuthenticatedImageSpec(model, null, null, allowHardware = true),
            ),
            imageLoader = imageSource.imageLoader(context),
            contentDescription = null,
            contentScale = contentScale,
            modifier = Modifier.fillMaxSize(),
            loading = { MomentoImagePlaceholder(loading = true) },
            error = { MomentoImagePlaceholder(loading = false) },
            success = { SubcomposeAsyncImageContent() },
        )
    }
}

@Composable
private fun MomentoImagePlaceholder(loading: Boolean) {
    Box(
        modifier = Modifier
            .fillMaxSize()
            .background(MaterialTheme.colorScheme.surfaceVariant),
        contentAlignment = Alignment.Center,
    ) {
        if (loading) {
            CircularProgressIndicator(
                modifier = Modifier.size(20.dp),
                strokeWidth = 2.dp,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        } else {
            Icon(
                imageVector = Icons.Outlined.ImageNotSupported,
                contentDescription = null,
                tint = MaterialTheme.colorScheme.onSurfaceVariant.copy(alpha = 0.55f),
            )
        }
    }
}
