package io.github.yzard.momento.core.data

import android.content.Context
import coil.ImageLoader

interface AuthenticatedMediaRepository {
    suspend fun thumbnailUrl(mediaId: Long, tiny: Boolean): String
    suspend fun trashThumbnailUrl(mediaId: Long): String
    fun authenticatedImageLoader(context: Context): ImageLoader
}
