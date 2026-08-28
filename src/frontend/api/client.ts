import axios, { type AxiosError, type AxiosResponse, type InternalAxiosRequestConfig } from 'axios'

type AuthenticationFailureHandler = () => void

let authenticationFailureHandler: AuthenticationFailureHandler | null = null

export function setAuthenticationFailureHandler(handler: AuthenticationFailureHandler | null) {
  authenticationFailureHandler = handler
}

export const apiClient = axios.create({
  baseURL: '/api/v1',
  headers: {
    'Content-Type': 'application/json',
  },
  withCredentials: true,
})

let isRefreshing = false
let failedQueue: Array<{
  resolve: (value: unknown) => void
  reject: (reason?: unknown) => void
}> = []

const processQueue = (error: Error | null) => {
  failedQueue.forEach((pendingRequest) => {
    if (error) {
      pendingRequest.reject(error)
    } else {
      pendingRequest.resolve(undefined)
    }
  })
  failedQueue = []
}

function isBrowserSessionRequest(config: InternalAxiosRequestConfig): boolean {
  return config.url?.includes('/user/session/') ?? false
}

interface RetriableRequest extends InternalAxiosRequestConfig {
  _retry?: boolean
}

function isPasswordChangeRequired(error: AxiosError<{ code?: string }>): boolean {
  return error.response?.status === 403 && error.response.data?.code === 'password_change_required'
}

function shouldRefreshSession(error: AxiosError, request: RetriableRequest): boolean {
  return error.response?.status === 401 && !request._retry && !isBrowserSessionRequest(request)
}

function retryAfterCurrentRefresh(request: RetriableRequest): Promise<AxiosResponse> {
  return new Promise((resolve, reject) => {
    failedQueue.push({ resolve, reject })
  }).then(() => apiClient(request))
}

async function refreshSessionAndRetry(request: RetriableRequest): Promise<AxiosResponse> {
  request._retry = true
  isRefreshing = true

  try {
    await axios.post('/api/v1/user/session/refresh', null, { withCredentials: true })
    processQueue(null)
    return apiClient(request)
  } catch (refreshError) {
    processQueue(refreshError as Error)
    authenticationFailureHandler?.()
    throw refreshError
  } finally {
    isRefreshing = false
  }
}

async function handleResponseError(error: AxiosError<{ code?: string }>) {
  const originalRequest = error.config as RetriableRequest

  if (isPasswordChangeRequired(error)) {
    authenticationFailureHandler?.()
    throw error
  }

  if (!shouldRefreshSession(error, originalRequest)) throw error
  if (isRefreshing) return retryAfterCurrentRefresh(originalRequest)
  return refreshSessionAndRetry(originalRequest)
}

apiClient.interceptors.response.use((response) => response, handleResponseError)
