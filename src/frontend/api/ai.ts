import { apiClient } from './client'

export interface AiTaskStatus {
  status: string
  queuedJobs: number
  processingJobs: number
  completedJobs: number
  failedJobs: number
  errors: string[]
}

export const aiApi = {
  trigger: async (): Promise<{ message: string; queuedJobs: number }> => (await apiClient.post('/ai/trigger', {})).data,
  triggerOcr: async (): Promise<{ message: string; queuedJobs: number }> => (await apiClient.post('/ai/ocr/trigger', {})).data,
  triggerImageTagging: async (): Promise<{ message: string; queuedJobs: number }> => (await apiClient.post('/ai/image_tagging/trigger', {})).data,
  getOcrStatus: async (): Promise<AiTaskStatus> => (await apiClient.post('/ai/ocr/status', {})).data,
  getImageTaggingStatus: async (): Promise<AiTaskStatus> => (await apiClient.post('/ai/image_tagging/status', {})).data,
}
