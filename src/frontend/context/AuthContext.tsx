import {
  createContext,
  useState,
  useEffect,
  useCallback,
  useRef,
  type Dispatch,
  type MutableRefObject,
  type ReactNode,
  type SetStateAction,
} from 'react'
import { useNavigate, type NavigateFunction } from 'react-router-dom'
import { authApi, type User } from '../api/auth'
import { setAuthenticationFailureHandler } from '../api/client'
import { facesApi } from '../api/faces'
import { mediaApi } from '../api/media'
import { queryClient } from '../lib/queryClient'

interface AuthContextType {
  user: User | null
  isAuthenticated: boolean
  isLoading: boolean
  login: (username: string, password: string) => Promise<User>
  logout: () => Promise<void>
  changePassword: (currentPassword: string, newPassword: string) => Promise<void>
  refreshUser: () => Promise<void>
}

const AuthContext = createContext<AuthContextType | null>(null)

interface AuthenticationActionsOptions {
  navigate: NavigateFunction
  sessionGeneration: MutableRefObject<number>
  setUser: Dispatch<SetStateAction<User | null>>
  clearSession: () => void
}

function useAuthenticationActions(options: AuthenticationActionsOptions) {
  const refreshUser = useCallback(async () => {
    const generation = options.sessionGeneration.current
    const userData = await authApi.getMe()
    if (generation !== options.sessionGeneration.current) return
    options.setUser(userData)
  }, [options])

  const login = useCallback(
    async (username: string, password: string) => {
      const generation = options.sessionGeneration.current + 1
      options.sessionGeneration.current = generation
      await authApi.login(username, password)
      if (generation !== options.sessionGeneration.current)
        throw new Error('Authentication request was superseded')
      let userData: User
      try {
        userData = await authApi.getMe()
      } catch (error) {
        if (generation !== options.sessionGeneration.current)
          throw new Error('Authentication request was superseded')
        try {
          await authApi.logout()
        } finally {
          options.clearSession()
        }
        throw error
      }
      if (generation !== options.sessionGeneration.current)
        throw new Error('Authentication request was superseded')
      options.setUser(userData)
      return userData
    },
    [options]
  )

  const logout = useCallback(async () => {
    options.clearSession()
    try {
      await authApi.logout()
    } catch {
      // The local session is already cleared.
    }
  }, [options])

  const changePassword = useCallback(
    async (currentPassword: string, newPassword: string) => {
      await authApi.changePassword(currentPassword, newPassword)
      options.clearSession()
      options.navigate('/login', { replace: true })
    },
    [options]
  )

  return { login, logout, changePassword, refreshUser }
}

function AuthProvider({ children }: { children: ReactNode }) {
  const navigate = useNavigate()
  const [user, setUser] = useState<User | null>(null)
  const [isLoading, setIsLoading] = useState(true)
  const sessionGeneration = useRef(0)

  const clearSession = useCallback(() => {
    sessionGeneration.current += 1
    setUser(null)
    setIsLoading(false)
    queryClient.clear()
    mediaApi.clearCache()
    facesApi.clearThumbnailCache()
  }, [])

  useEffect(() => {
    setAuthenticationFailureHandler(() => {
      clearSession()
      navigate('/login', { replace: true })
    })

    return () => setAuthenticationFailureHandler(null)
  }, [clearSession, navigate])

  const fetchUser = useCallback(async () => {
    const generation = sessionGeneration.current
    try {
      const userData = await authApi.getMe()
      if (generation !== sessionGeneration.current) return
      setUser(userData)
    } catch {
      if (generation !== sessionGeneration.current) return
      setUser(null)
    } finally {
      if (generation === sessionGeneration.current) setIsLoading(false)
    }
  }, [])

  useEffect(() => {
    fetchUser()
  }, [fetchUser])
  const actions = useAuthenticationActions({ navigate, sessionGeneration, setUser, clearSession })

  return (
    <AuthContext.Provider
      value={{
        user,
        isAuthenticated: !!user,
        isLoading,
        ...actions,
      }}
    >
      {children}
    </AuthContext.Provider>
  )
}

export { AuthContext, AuthProvider }
