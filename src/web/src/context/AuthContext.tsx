import { createContext, useState, useEffect, useCallback, type ReactNode } from 'react'
import { authApi, type TokenResponse } from '../api/auth'
import { mediaApi } from '../api/media'
import { queryClient } from '../lib/queryClient'

const ACCESS_TOKEN_KEY = 'momento_access_token'
const REFRESH_TOKEN_KEY = 'momento_refresh_token'

interface User {
  id: number
  username: string
  email: string
  role: 'admin' | 'user'
  mustChangePassword: boolean
}

interface AuthContextType {
  user: User | null
  isAuthenticated: boolean
  isLoading: boolean
  login: (username: string, password: string) => Promise<User>
  logout: () => Promise<void>
  refreshToken: () => Promise<boolean>
  refreshUser: () => Promise<void>
}

const AuthContext = createContext<AuthContextType | null>(null)

function AuthProvider({ children }: { children: ReactNode }) {
  const [user, setUser] = useState<User | null>(null)
  const [isLoading, setIsLoading] = useState(true)

  const saveTokens = (tokens: TokenResponse) => {
    localStorage.setItem(ACCESS_TOKEN_KEY, tokens.accessToken)
    localStorage.setItem(REFRESH_TOKEN_KEY, tokens.refreshToken)
  }

  const clearTokens = () => {
    localStorage.removeItem(ACCESS_TOKEN_KEY)
    localStorage.removeItem(REFRESH_TOKEN_KEY)
  }

  const refreshToken = useCallback(async (): Promise<boolean> => {
    const storedRefreshToken = localStorage.getItem(REFRESH_TOKEN_KEY)
    if (!storedRefreshToken) return false

    try {
      const tokens = await authApi.refresh(storedRefreshToken)
      saveTokens(tokens)
      return true
    } catch {
      clearTokens()
      setUser(null)
      return false
    }
  }, [])

  const fetchUser = useCallback(async () => {
    const accessToken = localStorage.getItem(ACCESS_TOKEN_KEY)
    if (!accessToken) {
      setIsLoading(false)
      return
    }

    try {
      const userData = await authApi.getMe()
      setUser(userData)
    } catch {
      const refreshed = await refreshToken()
      if (refreshed) {
        try {
          const userData = await authApi.getMe()
          setUser(userData)
        } catch {
          clearTokens()
        }
      }
    } finally {
      setIsLoading(false)
    }
  }, [refreshToken])

  const refreshUser = useCallback(async () => {
    const userData = await authApi.getMe()
    setUser(userData)
  }, [])

  useEffect(() => {
    fetchUser()
  }, [fetchUser])

  const login = async (username: string, password: string) => {
    const tokens = await authApi.login(username, password)
    saveTokens(tokens)
    const userData = await authApi.getMe()
    setUser(userData)
    return userData
  }

  const logout = async () => {
    const refreshTokenValue = localStorage.getItem(REFRESH_TOKEN_KEY)
    if (refreshTokenValue) {
      try {
        await authApi.logout(refreshTokenValue)
      } catch {
        // Ignore logout errors
      }
    }
    clearTokens()
    setUser(null)
    
    // Clear all cached data to prevent data leakage between users
    queryClient.clear()
    mediaApi.clearCache()
  }

  return (
    <AuthContext.Provider
      value={{
        user,
        isAuthenticated: !!user,
        isLoading,
        login,
        logout,
        refreshToken,
        refreshUser,
      }}
    >
      {children}
    </AuthContext.Provider>
  )
}

export { AuthContext, AuthProvider }
