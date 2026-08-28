package io.github.yzard.momento.core.data

import io.github.yzard.momento.core.model.MessageResponse

interface AccountRepository {
    suspend fun changePassword(current: String, updated: String): MessageResponse
}
