package io.github.yzard.momento.core.ui

import android.content.Context
import android.graphics.Bitmap
import androidx.core.graphics.drawable.toBitmap
import coil.ImageLoader
import coil.request.ImageRequest
import coil.request.SuccessResult
import io.github.yzard.momento.core.data.AuthenticatedMediaRepository

data class AuthenticatedImageSpec(
    val model: Any?,
    val widthPx: Int?,
    val heightPx: Int?,
    val allowHardware: Boolean,
)

fun squareAuthenticatedImageSpec(
    model: Any?,
    sizePx: Int,
    allowHardware: Boolean,
): AuthenticatedImageSpec {
    require(sizePx > 0) { "Image size must be positive" }
    return AuthenticatedImageSpec(model, sizePx, sizePx, allowHardware)
}

class AuthenticatedImageSource(
    private val repository: AuthenticatedMediaRepository,
) {
    fun imageLoader(context: Context): ImageLoader = repository.authenticatedImageLoader(context)

    fun request(context: Context, spec: AuthenticatedImageSpec): ImageRequest {
        val builder = ImageRequest.Builder(context)
            .data(spec.model)
            .allowHardware(spec.allowHardware)
        if (spec.widthPx != null && spec.heightPx != null) {
            builder.size(spec.widthPx, spec.heightPx)
        }
        return builder.build()
    }

    suspend fun loadBitmap(context: Context, spec: AuthenticatedImageSpec): Bitmap? {
        val result = imageLoader(context).execute(request(context, spec)) as? SuccessResult ?: return null
        val width = result.drawable.intrinsicWidth.coerceAtLeast(1)
        val height = result.drawable.intrinsicHeight.coerceAtLeast(1)
        return result.drawable.toBitmap(width, height, Bitmap.Config.ARGB_8888)
    }
}
