package io.github.yzard.momento.app

import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import io.github.yzard.momento.core.data.EncryptedTokenStore
import io.github.yzard.momento.core.data.MomentoRepository
import io.github.yzard.momento.core.data.SettingsStore
import io.github.yzard.momento.core.network.NetworkClient
import io.github.yzard.momento.feature.map.initializeOpenStreetMap

class MainActivity : ComponentActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        enableEdgeToEdge()
        initializeOpenStreetMap(this)
        val settingsStore = SettingsStore(this)
        val tokenStore = EncryptedTokenStore(this)
        val repository = MomentoRepository(settingsStore, tokenStore, NetworkClient(tokenStore))
        setContent { MomentoApplication(settingsStore, repository, tokenStore) }
    }
}
