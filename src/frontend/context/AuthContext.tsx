import { createContext, useState, useEffect, useCallback, useRef, type ReactNode } from 'react'
import { useNavigate } from 'react-router-dom'
import { authApi, type TokenResponse, type User } from '../api/auth'
import { setForbiddenResponseHandler } from '../api/client'
import { mediaApi } from '../api/media'
import { queryClient } from '../lib/queryClient'

const ACCESS_TOKEN_KEY = 'momento_access_token'
const REFRESH_TOKEN_KEY = 'momento_refresh_token'

function saveTokens(tokens: TokenResponse) {
  localStorage.setItem(ACCESS_TOKEN_KEY, tokens.accessToken)
  localStorage.setItem(REFRESH_TOKEN_KEY, tokens.refreshToken)
}

function clearTokens() {
  localStorage.removeItem(ACCESS_TOKEN_KEY)
  localStorage.removeItem(REFRESH_TOKEN_KEY)
}

interface AuthContextType {
  user: User | null
  isAuthenticated: boolean
  isLoading: boolean
  login: (username: string, password: string) => Promise<User>
  logout: () => Promise<void>
  changePassword: (currentPassword: string, newPassword: string) => Promise<void>
  refreshToken: () => Promise<boolean>
  refreshUser: () => Promise<void>
}

const AuthContext = createContext<AuthContextType | null>(null)

function AuthProvider({ children }: { children: ReactNode }) {
  const navigate = useNavigate()
  const [user, setUser] = useState<User | null>(null)
  const [isLoading, setIsLoading] = useState(true)
  const sessionGeneration = useRef(0)

  const clearSession = useCallback(() => {
    sessionGeneration.current += 1
    clearTokens()
    setUser(null)
    setIsLoading(false)
    queryClient.clear()
    mediaApi.clearCache()
  }, [])

  useEffect(() => {
    setForbiddenResponseHandler(() => {
      clearSession()
      navigate('/login', { replace: true })
    })

    return () => setForbiddenResponseHandler(null)
  }, [clearSession, navigate])

  const refreshToken = useCallback(async (): Promise<boolean> => {
    const generation = sessionGeneration.current
    const storedRefreshToken = localStorage.getItem(REFRESH_TOKEN_KEY)
    if (!storedRefreshToken) return false

    try {
      const tokens = await authApi.refresh(storedRefreshToken)
      if (
        generation !== sessionGeneration.current
        || localStorage.getItem(REFRESH_TOKEN_KEY) !== storedRefreshToken
      ) return false
      saveTokens(tokens)
      return true
    } catch {
      if (
        generation !== sessionGeneration.current
        || localStorage.getItem(REFRESH_TOKEN_KEY) !== storedRefreshToken
      ) return false
      clearTokens()
      setUser(null)
      return false
    }
  }, [])

  const fetchUser = useCallback(async () => {
    const generation = sessionGeneration.current
    const accessToken = localStorage.getItem(ACCESS_TOKEN_KEY)
    if (!accessToken) {
      if (generation === sessionGeneration.current) setIsLoading(false)
      return
    }

    try {
      const userData = await authApi.getMe()
      if (generation !== sessionGeneration.current) return
      setUser(userData)
    } catch {
      if (generation !== sessionGeneration.current) return
      const refreshed = await refreshToken()
      if (refreshed) {
        try {
          const userData = await authApi.getMe()
          if (generation !== sessionGeneration.current) return
          setUser(userData)
        } catch {
          if (generation === sessionGeneration.current) clearTokens()
        }
      }
    } finally {
      if (generation === sessionGeneration.current) setIsLoading(false)
    }
  }, [refreshToken])

  const refreshUser = useCallback(async () => {
    const generation = sessionGeneration.current
    const userData = await authApi.getMe()
    if (generation !== sessionGeneration.current) return
    setUser(userData)
  }, [])

  useEffect(() => {
    fetchUser()
  }, [fetchUser])

  const login = async (username: string, password: string) => {
    const generation = sessionGeneration.current + 1
    sessionGeneration.current = generation
    const tokens = await authApi.login(username, password)
    if (generation !== sessionGeneration.current) {
      throw new Error('Authentication request was superseded')
    }
    saveTokens(tokens)
    const userData = await authApi.getMe()
    if (generation !== sessionGeneration.current) {
      throw new Error('Authentication request was superseded')
    }
    setUser(userData)
    return userData
  }

  const logout = async () => {
    const refreshTokenValue = localStorage.getItem(REFRESH_TOKEN_KEY)
    clearSession()
    if (refreshTokenValue) {
      try {
        await authApi.logout(refreshTokenValue)
      } catch {
        // Ignore logout errors
      }
    }
  }

  const changePassword = async (currentPassword: string, newPassword: string) => {
    await authApi.changePassword(currentPassword, newPassword)
    clearSession()
    navigate('/login', { replace: true })
  }

  return (
    <AuthContext.Provider
      value={{
        user,
        isAuthenticated: !!user,
        isLoading,
        login,
        logout,
        changePassword,
        refreshToken,
        refreshUser,
      }}
    >
      {children}
    </AuthContext.Provider>
  )
}

export { AuthContext, AuthProvider }
