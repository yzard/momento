import { afterEach, describe, expect, it, vi } from 'vitest'

import { authApi } from '../../../src/frontend/api/auth'
import { apiClient } from '../../../src/frontend/api/client'

afterEach(() => vi.restoreAllMocks())

describe('authApi browser sessions', () => {
  it('creates the cookie session with HTTP Basic credentials and no token payload', async () => {
    const post = vi.spyOn(apiClient, 'post').mockResolvedValue({ data: undefined })

    await authApi.login('member', 'password')

    expect(post).toHaveBeenCalledWith('/user/session/create', null, {
      auth: { username: 'member', password: 'password' },
    })
  })

  it('refreshes and deletes the cookie session through dedicated endpoints', async () => {
    const post = vi.spyOn(apiClient, 'post').mockResolvedValue({ data: undefined })

    await authApi.refresh()
    await authApi.logout()

    expect(post).toHaveBeenNthCalledWith(1, '/user/session/refresh')
    expect(post).toHaveBeenNthCalledWith(2, '/user/session/delete')
  })
})
