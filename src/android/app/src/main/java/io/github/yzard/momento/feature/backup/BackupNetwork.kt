package io.github.yzard.momento.feature.backup

import android.content.Context
import android.net.ConnectivityManager
import android.net.Network
import android.net.NetworkCapabilities
import kotlinx.coroutines.channels.awaitClose
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.callbackFlow
import kotlinx.coroutines.flow.distinctUntilChanged

internal fun isRetryable(statusCode: Int): Boolean =
    statusCode == 408 || statusCode == 429 || statusCode >= 500

internal fun backupNetworkAllowed(
    allowMobileData: Boolean,
    hasValidatedInternet: Boolean,
    unmetered: Boolean,
): Boolean = hasValidatedInternet && (allowMobileData || unmetered)

fun isBackupNetworkAllowed(context: Context, allowMobileData: Boolean): Boolean {
    val connectivityManager = context.getSystemService(ConnectivityManager::class.java)
    val capabilities = connectivityManager.getNetworkCapabilities(connectivityManager.activeNetwork)
        ?: return false
    return backupNetworkAllowed(
        allowMobileData = allowMobileData,
        hasValidatedInternet = capabilities.hasCapability(NetworkCapabilities.NET_CAPABILITY_INTERNET) &&
            capabilities.hasCapability(NetworkCapabilities.NET_CAPABILITY_VALIDATED),
        unmetered = capabilities.hasCapability(NetworkCapabilities.NET_CAPABILITY_NOT_METERED),
    )
}

fun observeBackupNetworkAllowed(context: Context, allowMobileData: Boolean): Flow<Boolean> =
    callbackFlow {
        val connectivityManager = context.getSystemService(ConnectivityManager::class.java)
        val callback = object : ConnectivityManager.NetworkCallback() {
            override fun onAvailable(network: Network) {
                trySend(isBackupNetworkAllowed(context, allowMobileData))
            }

            override fun onCapabilitiesChanged(network: Network, capabilities: NetworkCapabilities) {
                trySend(isBackupNetworkAllowed(context, allowMobileData))
            }

            override fun onLost(network: Network) {
                trySend(isBackupNetworkAllowed(context, allowMobileData))
            }
        }
        connectivityManager.registerDefaultNetworkCallback(callback)
        trySend(isBackupNetworkAllowed(context, allowMobileData))
        awaitClose { connectivityManager.unregisterNetworkCallback(callback) }
    }.distinctUntilChanged()
