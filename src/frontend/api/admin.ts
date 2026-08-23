import { apiClient } from './client'

interface User {
  id: number
  username: string
  email: string
  role: 'admin' | 'user'
  mustChangePassword: boolean
  isActive: boolean
  createdAt: string
}

export const adminApi = {
  listUsers: async (): Promise<User[]> => {
    const response = await apiClient.post<{ users: User[] }>('/user/list')
    return response.data.users
  },

  createUser: async (data: { username: string; email: string; password: string; role?: 'admin' | 'user' }): Promise<User> => {
    const response = await apiClient.post<User>('/user/create', data)
    return response.data
  },

  updateUser: async (data: { userId: number; role?: 'admin' | 'user'; isActive?: boolean }): Promise<User> => {
    const response = await apiClient.post<User>('/user/update', data)
    return response.data
  },

  deleteUser: async (userId: number): Promise<void> => {
    await apiClient.post('/user/delete', { userId })
  },

}
