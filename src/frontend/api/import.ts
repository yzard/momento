import { apiClient } from './client'

export interface ImportStatus {
  status: string
  totalFiles: number
  processedFiles: number
  successfulImports: number
  failedImports: number
  startedAt: string | null
  completedAt: string | null
  errors: string[]
}

export const importApi = {
  triggerLocal: async (): Promise<{ message: string; status: string }> => (await apiClient.post('/import/local', {})).data,
  getStatus: async (): Promise<ImportStatus> => (await apiClient.post('/import/status', {})).data,
}
