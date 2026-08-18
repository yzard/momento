import type { InternalAxiosRequestConfig } from 'axios'
import { act, cleanup, render, screen, waitFor } from '@testing-library/react'
import { MemoryRouter, useLocation } from 'react-router-dom'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

const mocks = vi.hoisted(() => ({
  clearMediaCache: vi.fn(),
  clearQueryCache: vi.fn(),
  getMe: vi.fn(),
  changePassword: vi.fn(),
}))

vi.mock('../../../src/frontend/api/auth', () => ({
  authApi: {
    getMe: mocks.getMe,
    changePassword: mocks.changePassword,
  },
}))

vi.mock('../../../src/frontend/api/media', () => ({
  mediaApi: {
    clearCache: mocks.clearMediaCache,
  },
}))

vi.mock('../../../src/frontend/lib/queryClient', () => ({
  queryClient: {
    clear: mocks.clearQueryCache,
  },
}))

import { apiClient } from '../../../src/frontend/api/client'
import { AuthProvider } from '../../../src/frontend/context/AuthContext'
import { useAuth } from '../../../src/frontend/hooks/useAuth'

const storedValues = new Map<string, string>()
const testLocalStorage = {
  clear: () => storedValues.clear(),
  getItem: (key: string) => storedValues.get(key) ?? null,
  removeItem: (key: string) => storedValues.delete(key),
  setItem: (key: string, value: string) => storedValues.set(key, value),
}

function SessionState() {
  const { changePassword, isAuthenticated, isLoading } = useAuth()
  const location = useLocation()

  return (
    <div>
      <span>{isLoading ? 'loading' : isAuthenticated ? 'authenticated' : 'logged-out'}</span>
      <span>{location.pathname}</span>
      <button type="button" onClick={() => void changePassword('old-password', 'new-password')}>
        Change password
      </button>
    </div>
  )
}

function rejectedForbiddenResponse(config: InternalAxiosRequestConfig) {
  return Promise.reject({
    config,
    isAxiosError: true,
    response: {
      config,
      data: {},
      headers: {},
      status: 403,
      statusText: 'Forbidden',
    },
  })
}

beforeEach(() => {
  vi.stubGlobal('localStorage', testLocalStorage)
  localStorage.setItem('momento_access_token', 'access-token')
  localStorage.setItem('momento_refresh_token', 'refresh-token')
  mocks.getMe.mockResolvedValue({
    id: 1,
    username: 'admin',
    email: 'admin@example.com',
    role: 'admin',
    mustChangePassword: false,
  })
})

afterEach(() => {
  cleanup()
  localStorage.clear()
  vi.clearAllMocks()
  vi.unstubAllGlobals()
})

describe('AuthProvider', () => {
  it('clears the session and redirects to login after a 403 response', async () => {
    render(
      <MemoryRouter initialEntries={['/timeline']}>
        <AuthProvider>
          <SessionState />
        </AuthProvider>
      </MemoryRouter>,
    )

    await screen.findByText('authenticated')

    await act(async () => {
      await apiClient.get('/protected', { adapter: rejectedForbiddenResponse }).catch(() => undefined)
    })

    await waitFor(() => {
      expect(screen.getByText('logged-out')).toBeTruthy()
      expect(screen.getByText('/login')).toBeTruthy()
    })
    expect(localStorage.getItem('momento_access_token')).toBeNull()
    expect(localStorage.getItem('momento_refresh_token')).toBeNull()
    expect(mocks.clearQueryCache).toHaveBeenCalledOnce()
    expect(mocks.clearMediaCache).toHaveBeenCalledOnce()
  })

  it('ends the current session immediately after a password change', async () => {
    mocks.changePassword.mockResolvedValue(undefined)
    render(
      <MemoryRouter initialEntries={['/settings']}>
        <AuthProvider>
          <SessionState />
        </AuthProvider>
      </MemoryRouter>,
    )

    await screen.findByText('authenticated')
    await act(async () => {
      screen.getByRole('button', { name: 'Change password' }).click()
    })

    await waitFor(() => {
      expect(screen.getByText('logged-out')).toBeTruthy()
      expect(screen.getByText('/login')).toBeTruthy()
    })
    expect(mocks.changePassword).toHaveBeenCalledWith('old-password', 'new-password')
    expect(localStorage.getItem('momento_access_token')).toBeNull()
    expect(localStorage.getItem('momento_refresh_token')).toBeNull()
    expect(mocks.clearQueryCache).toHaveBeenCalledOnce()
    expect(mocks.clearMediaCache).toHaveBeenCalledOnce()
  })
})
