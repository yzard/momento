import type { InternalAxiosRequestConfig } from 'axios'
import { act, cleanup, render, screen, waitFor } from '@testing-library/react'
import { MemoryRouter, useLocation } from 'react-router-dom'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

const mocks = vi.hoisted(() => ({
  clearQueryCache: vi.fn(),
  login: vi.fn(),
  getMe: vi.fn(),
  changePassword: vi.fn(),
  logout: vi.fn(),
}))

vi.mock('../../../src/frontend/api/auth', () => ({
  authApi: {
    login: mocks.login,
    getMe: mocks.getMe,
    changePassword: mocks.changePassword,
    logout: mocks.logout,
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

function SessionState() {
  const { changePassword, isAuthenticated, isLoading, login, logout } = useAuth()
  const location = useLocation()

  return (
    <div>
      <span>{isLoading ? 'loading' : isAuthenticated ? 'authenticated' : 'logged-out'}</span>
      <span>{location.pathname}</span>
      <button type="button" onClick={() => void changePassword('old-password', 'new-password')}>
        Change password
      </button>
      <button type="button" onClick={() => void logout()}>
        Log out
      </button>
      <button type="button" onClick={() => void login('admin', 'password').catch(() => undefined)}>
        Log in
      </button>
    </div>
  )
}

function rejectedPasswordChangeResponse(config: InternalAxiosRequestConfig) {
  return Promise.reject({
    config,
    isAxiosError: true,
    response: {
      config,
      data: { code: 'password_change_required' },
      headers: {},
      status: 403,
      statusText: 'Forbidden',
    },
  })
}

beforeEach(() => {
  mocks.getMe.mockResolvedValue({
    id: 1,
    username: 'admin',
    email: 'admin@example.com',
    role: 'admin',
    isReserved: true,
    mustChangePassword: false,
  })
  mocks.logout.mockResolvedValue(undefined)
  mocks.login.mockResolvedValue(undefined)
})

afterEach(() => {
  cleanup()
  vi.clearAllMocks()
})

describe('AuthProvider', () => {
  it('clears the session and redirects to login after a 403 response', async () => {
    render(
      <MemoryRouter initialEntries={['/timeline']}>
        <AuthProvider>
          <SessionState />
        </AuthProvider>
      </MemoryRouter>
    )

    await screen.findByText('authenticated')

    await act(async () => {
      await apiClient
        .get('/protected', { adapter: rejectedPasswordChangeResponse })
        .catch(() => undefined)
    })

    await waitFor(() => {
      expect(screen.getByText('logged-out')).toBeTruthy()
      expect(screen.getByText('/login')).toBeTruthy()
    })
    expect(mocks.clearQueryCache).toHaveBeenCalledOnce()
  })

  it('ends the current session immediately after a password change', async () => {
    mocks.changePassword.mockResolvedValue(undefined)
    render(
      <MemoryRouter initialEntries={['/settings']}>
        <AuthProvider>
          <SessionState />
        </AuthProvider>
      </MemoryRouter>
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
    expect(mocks.clearQueryCache).toHaveBeenCalledOnce()
  })

  it('does not restore a bootstrap user after logout', async () => {
    let resolveUser!: (user: unknown) => void
    mocks.getMe.mockReturnValue(
      new Promise((resolve) => {
        resolveUser = resolve
      })
    )
    render(
      <MemoryRouter initialEntries={['/timeline']}>
        <AuthProvider>
          <SessionState />
        </AuthProvider>
      </MemoryRouter>
    )
    await waitFor(() => expect(mocks.getMe).toHaveBeenCalledOnce())

    await act(async () => {
      screen.getByRole('button', { name: 'Log out' }).click()
    })
    await screen.findByText('logged-out')
    await act(async () =>
      resolveUser({
        id: 2,
        username: 'stale',
        email: 'stale@example.com',
        role: 'user',
        isReserved: false,
        mustChangePassword: false,
      })
    )

    expect(screen.getByText('logged-out')).toBeTruthy()
  })

  it('deletes the server session when loading the authenticated user fails during login', async () => {
    render(
      <MemoryRouter initialEntries={['/login']}>
        <AuthProvider>
          <SessionState />
        </AuthProvider>
      </MemoryRouter>
    )
    await screen.findByText('authenticated')

    mocks.getMe.mockRejectedValueOnce(new Error('profile failed'))

    await act(async () => {
      screen.getByRole('button', { name: 'Log in' }).click()
    })

    await waitFor(() => expect(mocks.getMe).toHaveBeenCalledTimes(2))
    expect(mocks.logout).toHaveBeenCalledOnce()
    expect(mocks.clearQueryCache).toHaveBeenCalledOnce()
  })
})
