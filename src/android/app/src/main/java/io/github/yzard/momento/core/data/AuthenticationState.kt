package io.github.yzard.momento.core.data

import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow

class AuthenticationState(initiallyAuthenticated: Boolean) {
    private val mutableAuthenticated = MutableStateFlow(initiallyAuthenticated)
    val isAuthenticated: StateFlow<Boolean> = mutableAuthenticated.asStateFlow()

    fun signedIn() {
        mutableAuthenticated.value = true
    }

    fun signedOut() {
        mutableAuthenticated.value = false
    }
}
