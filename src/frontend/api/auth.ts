import { apiClient } from './client'

export interface User {
  id: number
  username: string
  email: string
  role: 'admin' | 'user'
  isReserved: boolean
  mustChangePassword: boolean
  isActive: boolean
  createdAt: string
}

export const authApi = {
  login: async (username: string, password: string): Promise<void> => {
    await apiClient.post('/user/session/create', null, {
      auth: { username, password },
    })
  },

  refresh: async (): Promise<void> => {
    await apiClient.post('/user/session/refresh')
  },

  logout: async (): Promise<void> => {
    await apiClient.post('/user/session/delete')
  },

  getMe: async (): Promise<User> => {
    const response = await apiClient.post<User>('/user/get')
    return response.data
  },

  changePassword: async (currentPassword: string, newPassword: string): Promise<void> => {
    await apiClient.post('/user/change-password', {
      currentPassword,
      newPassword,
    })
  },
}
