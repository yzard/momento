import { apiClient } from './client'

export interface TokenResponse {
  access_token: string
  refresh_token: string
  token_type: string
}

interface User {
  id: number
  username: string
  email: string
  role: 'admin' | 'user'
  mustChangePassword: boolean
}

export const authApi = {
  login: async (username: string, password: string): Promise<TokenResponse> => {
    const response = await apiClient.post<TokenResponse>('/user/authenticate', null, {
      auth: { username, password },
    })
    return response.data
  },

  refresh: async (refreshToken: string): Promise<TokenResponse> => {
    const response = await apiClient.post<TokenResponse>('/user/refresh', {
      refresh_token: refreshToken,
    })
    return response.data
  },

  logout: async (refreshToken: string): Promise<void> => {
    await apiClient.post('/user/logout', { refresh_token: refreshToken })
  },

  getMe: async (): Promise<User> => {
    const response = await apiClient.post<User>('/user/get')
    return response.data
  },

  changePassword: async (currentPassword: string, newPassword: string): Promise<void> => {
    await apiClient.post('/user/change-password', {
      current_password: currentPassword,
      new_password: newPassword,
    })
  },
}
