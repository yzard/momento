import { apiClient } from './client'

interface User {
  id: number
  username: string
  email: string
  role: 'admin' | 'user'
  must_change_password: boolean
  is_active: boolean
  created_at: string
}

interface ImportStatus {
  status: string
  total_files: number
  processed_files: number
  successful_imports: number
  failed_imports: number
  started_at: string | null
  completed_at: string | null
  errors: string[]
}

interface RegenerationStatus {
  status: string
  total_media: number
  processed_media: number
  updated_metadata: number
  generated_thumbnails: number
  updated_tags: number
  started_at: string | null
  completed_at: string | null
  errors: string[]
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

  updateUser: async (userId: number, data: { role?: 'admin' | 'user'; is_active?: boolean }): Promise<User> => {
    const response = await apiClient.post<User>('/user/update', data, { params: { user_id: userId } })
    return response.data
  },

  deleteUser: async (userId: number): Promise<void> => {
    await apiClient.post('/user/delete', { user_id: userId })
  },

  triggerImport: async (): Promise<{ message: string; status: string }> => {
    const response = await apiClient.post<{ message: string; status: string }>('/import/local')
    return response.data
  },

  triggerWebdavImport: async (): Promise<{ message: string; status: string }> => {
    const response = await apiClient.post<{ message: string; status: string }>('/import/webdav')
    return response.data
  },

  getImportStatus: async (): Promise<ImportStatus> => {
    const response = await apiClient.post<ImportStatus>('/import/status')
    return response.data
  },

  regenerateMedia: async (missingOnly: boolean): Promise<{ message: string; status: string }> => {
    const response = await apiClient.post<{ message: string; status: string }>('/import/regenerate', {
      missing_only: missingOnly,
    })
    return response.data
  },

  resetLibrary: async (): Promise<{ message: string; status: string }> => {
    const response = await apiClient.post<{ message: string; status: string }>('/import/reset')
    return response.data
  },

  getRegenerationStatus: async (): Promise<RegenerationStatus> => {
    const response = await apiClient.post<RegenerationStatus>('/import/regenerate/status')
    return response.data
  },

  cancelRegeneration: async (): Promise<{ message: string; status: string }> => {
    const response = await apiClient.post<{ message: string; status: string }>('/import/regenerate/cancel')
    return response.data
  },
}
