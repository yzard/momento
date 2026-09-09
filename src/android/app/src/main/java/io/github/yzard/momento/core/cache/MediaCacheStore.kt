package io.github.yzard.momento.core.cache

import android.content.Context
import android.net.ConnectivityManager
import android.net.NetworkCapabilities
import java.io.File
import java.io.IOException
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.withContext
import okhttp3.Interceptor

class MediaCacheStore private constructor(context: Context) {
    private val preferences = context.getSharedPreferences("momento_media_cache", Context.MODE_PRIVATE)
    private val selected = MutableStateFlow(parseMediaCacheLimit(preferences.getString("limit", null)))
    val limit = selected.asStateFlow()
    private val disk = DiskMediaCache(File(context.cacheDir, "media-cache-v1"), selected.value.bytes)
    private val connectivity = context.getSystemService(ConnectivityManager::class.java)

    suspend fun setLimit(limit: MediaCacheLimit) = withContext(Dispatchers.IO) {
        disk.resize(limit.bytes)
        if (!preferences.edit().putString("limit", limit.name).commit()) throw IOException("Could not save cache size")
        selected.value = limit
    }

    fun interceptor(namespace: () -> String?): Interceptor = MediaCacheInterceptor(disk, namespace, {
        val network = connectivity.activeNetwork
        network != null && connectivity.getNetworkCapabilities(network)?.hasCapability(NetworkCapabilities.NET_CAPABILITY_INTERNET) == true
    }, System::currentTimeMillis)

    companion object {
        @Volatile private var instance: MediaCacheStore? = null
        fun get(context: Context): MediaCacheStore = instance ?: synchronized(this) {
            instance ?: MediaCacheStore(context.applicationContext).also { instance = it }
        }
    }
}
