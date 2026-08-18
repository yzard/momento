import { apiClient } from './client'

export interface AiTaskStatus {
  status: string
  queuedJobs: number
  processingJobs: number
  completedJobs: number
  failedJobs: number
  errors: string[]
}

export interface FaceTaskStatus extends AiTaskStatus {
  faceGroups: number
}

export const aiApi = {
  trigger: async (): Promise<{ message: string; queuedJobs: number }> => (await apiClient.post('/ai/trigger', {})).data,
  cancel: async (): Promise<{ message: string; queuedJobs: number }> => (await apiClient.post('/ai/cancel', {})).data,
  clean: async (): Promise<{ message: string; queuedJobs: number }> => (await apiClient.post('/ai/clean', {})).data,
  triggerOcr: async (): Promise<{ message: string; queuedJobs: number }> => (await apiClient.post('/ai/ocr/trigger', {})).data,
  cancelOcr: async (): Promise<{ message: string; queuedJobs: number }> => (await apiClient.post('/ai/ocr/cancel', {})).data,
  cleanOcr: async (): Promise<{ message: string; queuedJobs: number }> => (await apiClient.post('/ai/ocr/clean', {})).data,
  triggerImageTagging: async (): Promise<{ message: string; queuedJobs: number }> => (await apiClient.post('/ai/image_tagging/trigger', {})).data,
  cancelImageTagging: async (): Promise<{ message: string; queuedJobs: number }> => (await apiClient.post('/ai/image_tagging/cancel', {})).data,
  cleanImageTagging: async (): Promise<{ message: string; queuedJobs: number }> => (await apiClient.post('/ai/image_tagging/clean', {})).data,
  triggerImageAesthetics: async (): Promise<{ message: string; queuedJobs: number }> => (await apiClient.post('/ai/image_aesthetics/trigger', {})).data,
  cancelImageAesthetics: async (): Promise<{ message: string; queuedJobs: number }> => (await apiClient.post('/ai/image_aesthetics/cancel', {})).data,
  cleanImageAesthetics: async (): Promise<{ message: string; queuedJobs: number }> => (await apiClient.post('/ai/image_aesthetics/clean', {})).data,
  triggerImageClustering: async (): Promise<{ message: string; queuedJobs: number }> => (await apiClient.post('/ai/image_clustering/trigger', {})).data,
  cancelImageClustering: async (): Promise<{ message: string; queuedJobs: number }> => (await apiClient.post('/ai/image_clustering/cancel', {})).data,
  cleanImageClustering: async (): Promise<{ message: string; queuedJobs: number }> => (await apiClient.post('/ai/image_clustering/clean', {})).data,
  startFaces: async (): Promise<{ message: string; queuedJobs: number }> => (await apiClient.post('/ai/faces/start', {})).data,
  cancelFaces: async (): Promise<{ message: string; queuedJobs: number }> => (await apiClient.post('/ai/faces/cancel', {})).data,
  cleanFaces: async (): Promise<{ message: string; queuedJobs: number }> => (await apiClient.post('/ai/faces/clean', {})).data,
  getOcrStatus: async (): Promise<AiTaskStatus> => (await apiClient.post('/ai/ocr/status', {})).data,
  getImageTaggingStatus: async (): Promise<AiTaskStatus> => (await apiClient.post('/ai/image_tagging/status', {})).data,
  getImageAestheticsStatus: async (): Promise<AiTaskStatus> => (await apiClient.post('/ai/image_aesthetics/status', {})).data,
  getFacesStatus: async (): Promise<FaceTaskStatus> => (await apiClient.post('/ai/faces/status', {})).data,
}
