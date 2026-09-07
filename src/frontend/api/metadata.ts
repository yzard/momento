import { apiClient } from './client'

export interface MetadataStatus {
  status: string
  queuedJobs: number
  waitingForRollbackJobs: number
  processingJobs: number
  cancellingJobs: number
  completedJobs: number
  failedJobs: number
  errors: string[]
}

export interface MetadataActionResponse {
  message: string
  affectedJobs: number
}

export const metadataApi = {
  generate: async (): Promise<MetadataActionResponse> =>
    (await apiClient.post('/metadata/generate', {})).data,
  cancel: async (): Promise<MetadataActionResponse> =>
    (await apiClient.post('/metadata/cancel', {})).data,
  getStatus: async (): Promise<MetadataStatus> =>
    (await apiClient.post('/metadata/status', {})).data,
  clean: async (): Promise<MetadataActionResponse> =>
    (await apiClient.post('/metadata/clean', {})).data,
}
