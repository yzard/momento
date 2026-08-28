package io.github.yzard.momento.core.data

import android.content.Context
import androidx.datastore.preferences.core.booleanPreferencesKey
import androidx.datastore.preferences.core.edit
import androidx.datastore.preferences.core.stringPreferencesKey
import androidx.datastore.preferences.preferencesDataStore
import io.github.yzard.momento.core.model.TokenPair
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.map
import javax.crypto.Cipher
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey
import javax.crypto.spec.GCMParameterSpec
import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyProperties
import java.security.KeyStore
import java.util.Base64
import java.util.UUID
import java.net.URI
import java.net.URISyntaxException
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.first

private val Context.preferences by preferencesDataStore("momento_settings")

enum class ThemePreference {
    SYSTEM,
    LIGHT,
    DARK,
}

fun parseThemePreference(value: String?): ThemePreference =
    ThemePreference.entries.firstOrNull { it.name == value } ?: ThemePreference.SYSTEM

internal fun newBackupGeneration(): String = UUID.randomUUID().toString().replace("-", "")

data class Settings(
    val origin: String?,
    val mobileDataEnabled: Boolean,
    val cameraOnly: Boolean,
    val themePreference: ThemePreference,
)

class SettingsStore(private val context: Context) {
    private val originKey = stringPreferencesKey("server_origin")
    private val mobileKey = booleanPreferencesKey("mobile_data_enabled")
    private val cameraKey = booleanPreferencesKey("camera_only")
    private val deviceIdKey = stringPreferencesKey("backup_device_id")
    private val backupGenerationKey = stringPreferencesKey("backup_generation")
    private val themeKey = stringPreferencesKey("theme")
    val settings: Flow<Settings> = context.preferences.data.map {
        Settings(
            origin = it[originKey],
            mobileDataEnabled = it[mobileKey] ?: false,
            cameraOnly = it[cameraKey] ?: true,
            themePreference = parseThemePreference(it[themeKey]),
        )
    }
    suspend fun setOrigin(origin: String, allowCleartextTraffic: Boolean) = context.preferences.edit {
        it[originKey] = normalizeServerOrigin(origin, allowCleartextTraffic)
    }
    suspend fun setMobileDataEnabled(enabled: Boolean) = context.preferences.edit { it[mobileKey] = enabled }
    suspend fun setCameraOnly(enabled: Boolean) = context.preferences.edit { it[cameraKey] = enabled }
    suspend fun setThemePreference(themePreference: ThemePreference) = context.preferences.edit {
        it[themeKey] = themePreference.name
    }
    suspend fun deviceId(): String {
        val existing = context.preferences.data.map { it[deviceIdKey] }.first()
        if (existing != null) return existing
        val generated = UUID.randomUUID().toString()
        context.preferences.edit { preferences ->
            if (preferences[deviceIdKey] == null) preferences[deviceIdKey] = generated
        }
        return context.preferences.data.map { it[deviceIdKey] }.first() ?: generated
    }
    suspend fun backupGeneration(): String? =
        context.preferences.data.map { it[backupGenerationKey] }.first()

    suspend fun rotateBackupGeneration(): String {
        val generation = newBackupGeneration()
        context.preferences.edit { it[backupGenerationKey] = generation }
        return generation
    }
}

fun normalizeServerOrigin(input: String, allowCleartextTraffic: Boolean): String {
    val candidate = input.trim().removeSuffix("/")
    val uri = try {
        URI(candidate)
    } catch (exception: URISyntaxException) {
        throw IllegalArgumentException("Enter a valid server origin", exception)
    }
    val scheme = uri.scheme?.lowercase()
    val validScheme = scheme == "https" || (allowCleartextTraffic && scheme == "http")
    require(validScheme) {
        if (allowCleartextTraffic) {
            "Server address must start with https:// or http://"
        } else {
            "HTTPS is required in release builds"
        }
    }
    require(!uri.host.isNullOrBlank()) { "Enter a valid server origin" }
    require(uri.path.isNullOrBlank()) { "Enter a server origin without a path" }
    require(uri.userInfo == null && uri.query == null && uri.fragment == null) {
        "Enter a server origin without credentials, a query, or a fragment"
    }
    return candidate
}

class EncryptedTokenStore(context: Context) {
    private val storage = context.getSharedPreferences("momento_secure", Context.MODE_PRIVATE)
    private val keyAlias = "momento_tokens"
    private val authenticationCompleteKey = "authentication_complete"
    private val authenticationState = AuthenticationState(
        authenticationCompleted(
            encryptedAccessToken = storage.getString("access", null),
            storedCompletion = if (storage.contains(authenticationCompleteKey)) {
                storage.getBoolean(authenticationCompleteKey, false)
            } else {
                null
            },
        ),
    )
    val isAuthenticated: StateFlow<Boolean> = authenticationState.isAuthenticated
    private fun key(): SecretKey {
        val keyStore = KeyStore.getInstance("AndroidKeyStore").apply { load(null) }
        val existing = keyStore.getKey(keyAlias, null) as? SecretKey
        if (existing != null) return existing
        return KeyGenerator.getInstance(KeyProperties.KEY_ALGORITHM_AES, "AndroidKeyStore").apply {
            init(KeyGenParameterSpec.Builder(keyAlias, KeyProperties.PURPOSE_ENCRYPT or KeyProperties.PURPOSE_DECRYPT).setBlockModes(KeyProperties.BLOCK_MODE_GCM).setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE).build())
        }.generateKey()
    }
    private fun encrypt(value: String): String {
        val cipher = Cipher.getInstance("AES/GCM/NoPadding").apply { init(Cipher.ENCRYPT_MODE, key()) }
        return Base64.getEncoder().encodeToString(cipher.iv + cipher.doFinal(value.toByteArray()))
    }
    private fun decrypt(value: String): String {
        val bytes = Base64.getDecoder().decode(value); val cipher = Cipher.getInstance("AES/GCM/NoPadding")
        cipher.init(Cipher.DECRYPT_MODE, key(), GCMParameterSpec(128, bytes.copyOfRange(0, 12)))
        return cipher.doFinal(bytes.copyOfRange(12, bytes.size)).decodeToString()
    }
    fun saveLoginTokens(tokens: TokenPair) {
        storage.edit()
            .putString("access", encrypt(tokens.accessToken))
            .putString("refresh", encrypt(tokens.refreshToken))
            .putBoolean(authenticationCompleteKey, false)
            .apply()
        authenticationState.signedOut()
    }
    fun replaceSessionTokens(tokens: TokenPair) {
        storage.edit()
            .putString("access", encrypt(tokens.accessToken))
            .putString("refresh", encrypt(tokens.refreshToken))
            .apply()
    }
    fun markAuthenticated() {
        storage.edit().putBoolean(authenticationCompleteKey, true).apply()
        authenticationState.signedIn()
    }
    fun markAuthenticationIncomplete() {
        storage.edit().putBoolean(authenticationCompleteKey, false).apply()
        authenticationState.signedOut()
    }
    fun accessToken(): String? = storage.getString("access", null)?.let(::decrypt)
    fun refreshToken(): String? = storage.getString("refresh", null)?.let(::decrypt)
    fun clear() {
        storage.edit().clear().apply()
        authenticationState.signedOut()
    }
}

fun authenticationCompleted(encryptedAccessToken: String?, storedCompletion: Boolean?): Boolean {
    if (encryptedAccessToken == null) return false
    return storedCompletion ?: true
}
