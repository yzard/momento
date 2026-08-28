import { apiClient } from './client'

export interface MetadataStatus {
  status: string
  queuedJobs: number
  processingJobs: number
  completedJobs: number
  failedJobs: number
  errors: string[]
}

export const metadataApi = {
  generate: async (): Promise<{ message: string; queuedJobs: number }> =>
    (await apiClient.post('/metadata/generate', {})).data,
  getStatus: async (): Promise<MetadataStatus> =>
    (await apiClient.post('/metadata/status', {})).data,
  reset: async (): Promise<{ message: string; queuedJobs: number }> =>
    (await apiClient.post('/metadata/reset', {})).data,
}
