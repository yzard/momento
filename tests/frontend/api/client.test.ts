import axios, { type InternalAxiosRequestConfig } from 'axios'
import { afterEach, describe, expect, it, vi } from 'vitest'

import { apiClient, setAuthenticationFailureHandler } from '../../../src/frontend/api/client'

function rejectedResponse(status: number, data: { code?: string }) {
  return (config: InternalAxiosRequestConfig) =>
    Promise.reject({
      config,
      isAxiosError: true,
      response: {
        config,
        data,
        headers: {},
        status,
        statusText: 'Forbidden',
      },
    })
}

afterEach(() => {
  setAuthenticationFailureHandler(null)
  vi.restoreAllMocks()
})

describe('apiClient', () => {
  it('notifies the authentication provider when password change is required', async () => {
    const authenticationFailureHandler = vi.fn()
    setAuthenticationFailureHandler(authenticationFailureHandler)

    await expect(
      apiClient.get('/protected', {
        adapter: rejectedResponse(403, { code: 'password_change_required' }),
      })
    ).rejects.toMatchObject({
      response: { status: 403 },
    })

    expect(authenticationFailureHandler).toHaveBeenCalledOnce()
  })

  it('does not end the session for an ordinary authorization failure', async () => {
    const authenticationFailureHandler = vi.fn()
    setAuthenticationFailureHandler(authenticationFailureHandler)

    await expect(
      apiClient.get('/protected', {
        adapter: rejectedResponse(403, {}),
      })
    ).rejects.toMatchObject({ response: { status: 403 } })

    expect(authenticationFailureHandler).not.toHaveBeenCalled()
  })

  it('refreshes an expired browser session with HttpOnly cookies', async () => {
    vi.spyOn(axios, 'post').mockResolvedValue({ data: {} })

    await expect(
      apiClient.get('/protected', {
        adapter: rejectedResponse(401, {}),
      })
    ).rejects.toMatchObject({ response: { status: 401 } })

    expect(axios.post).toHaveBeenCalledWith('/api/v1/user/session/refresh', null, {
      withCredentials: true,
    })
  })

  it('reports a failed browser session refresh', async () => {
    const authenticationFailureHandler = vi.fn()
    setAuthenticationFailureHandler(authenticationFailureHandler)
    vi.spyOn(axios, 'post').mockRejectedValue(new Error('refresh failed'))

    await expect(
      apiClient.get('/protected', {
        adapter: rejectedResponse(401, {}),
      })
    ).rejects.toThrow('refresh failed')

    expect(authenticationFailureHandler).toHaveBeenCalledOnce()
  })

  it('does not recursively refresh a browser session endpoint', async () => {
    vi.spyOn(axios, 'post')

    await expect(
      apiClient.post('/user/session/create', null, {
        adapter: rejectedResponse(401, {}),
      })
    ).rejects.toMatchObject({ response: { status: 401 } })

    expect(axios.post).not.toHaveBeenCalled()
  })
})
