import type { InternalAxiosRequestConfig } from 'axios'
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
})
