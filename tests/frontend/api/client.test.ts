import axios, { type InternalAxiosRequestConfig } from 'axios'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import { apiClient, setForbiddenResponseHandler } from '../../../src/frontend/api/client'

function rejectedResponse(status: number) {
  return (config: InternalAxiosRequestConfig) => Promise.reject({
    config,
    isAxiosError: true,
    response: {
      config,
      data: {},
      headers: {},
      status,
      statusText: 'Forbidden',
    },
  })
}

beforeEach(() => vi.stubGlobal('localStorage', {
  getItem: vi.fn().mockReturnValue(null),
}))

afterEach(() => {
  setForbiddenResponseHandler(null)
  vi.unstubAllGlobals()
})

describe('apiClient', () => {
  it('notifies the authentication provider when any API request returns 403', async () => {
    const forbiddenResponseHandler = vi.fn()
    setForbiddenResponseHandler(forbiddenResponseHandler)

    await expect(apiClient.get('/protected', { adapter: rejectedResponse(403) })).rejects.toMatchObject({
      response: { status: 403 },
    })

    expect(forbiddenResponseHandler).toHaveBeenCalledOnce()
  })

  it('does not apply an old refresh result after the session changes', async () => {
    const values = new Map([
      ['momento_access_token', 'old-access'],
      ['momento_refresh_token', 'old-refresh'],
    ])
    vi.stubGlobal('localStorage', {
      getItem: (key: string) => values.get(key) ?? null,
      setItem: (key: string, value: string) => values.set(key, value),
      removeItem: (key: string) => values.delete(key),
    })
    let resolveRefresh!: (value: unknown) => void
    const refresh = new Promise((resolve) => {
      resolveRefresh = resolve
    })
    vi.spyOn(axios, 'post').mockReturnValue(refresh as ReturnType<typeof axios.post>)
    const request = apiClient.get('/protected', { adapter: rejectedResponse(401) })
    await vi.waitFor(() => expect(axios.post).toHaveBeenCalledOnce())

    values.set('momento_access_token', 'new-access')
    values.set('momento_refresh_token', 'new-refresh')
    resolveRefresh({ data: { accessToken: 'stale-access', refreshToken: 'stale-refresh' } })

    await expect(request).rejects.toThrow('Authentication session changed during refresh')
    expect(values.get('momento_access_token')).toBe('new-access')
    expect(values.get('momento_refresh_token')).toBe('new-refresh')
  })
})
