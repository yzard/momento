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
  start: async (): Promise<{ message: string; queuedJobs: number }> => (await apiClient.post('/ai/start', {})).data,
  cancel: async (): Promise<{ message: string; queuedJobs: number }> => (await apiClient.post('/ai/cancel', {})).data,
  clean: async (): Promise<{ message: string; queuedJobs: number }> => (await apiClient.post('/ai/clean', {})).data,
  startOcr: async (): Promise<{ message: string; queuedJobs: number }> => (await apiClient.post('/ai/ocr/start', {})).data,
  cancelOcr: async (): Promise<{ message: string; queuedJobs: number }> => (await apiClient.post('/ai/ocr/cancel', {})).data,
  cleanOcr: async (): Promise<{ message: string; queuedJobs: number }> => (await apiClient.post('/ai/ocr/clean', {})).data,
  startImageTagging: async (): Promise<{ message: string; queuedJobs: number }> => (await apiClient.post('/ai/image_tagging/start', {})).data,
  cancelImageTagging: async (): Promise<{ message: string; queuedJobs: number }> => (await apiClient.post('/ai/image_tagging/cancel', {})).data,
  cleanImageTagging: async (): Promise<{ message: string; queuedJobs: number }> => (await apiClient.post('/ai/image_tagging/clean', {})).data,
  startScreenshotDetection: async (): Promise<{ message: string; queuedJobs: number }> => (await apiClient.post('/ai/screenshot_detection/start', {})).data,
  cancelScreenshotDetection: async (): Promise<{ message: string; queuedJobs: number }> => (await apiClient.post('/ai/screenshot_detection/cancel', {})).data,
  cleanScreenshotDetection: async (): Promise<{ message: string; queuedJobs: number }> => (await apiClient.post('/ai/screenshot_detection/clean', {})).data,
  startDocumentDetection: async (): Promise<{ message: string; queuedJobs: number }> => (await apiClient.post('/ai/document_detection/start', {})).data,
  cancelDocumentDetection: async (): Promise<{ message: string; queuedJobs: number }> => (await apiClient.post('/ai/document_detection/cancel', {})).data,
  cleanDocumentDetection: async (): Promise<{ message: string; queuedJobs: number }> => (await apiClient.post('/ai/document_detection/clean', {})).data,
  startImageAesthetics: async (): Promise<{ message: string; queuedJobs: number }> => (await apiClient.post('/ai/image_aesthetics/start', {})).data,
  cancelImageAesthetics: async (): Promise<{ message: string; queuedJobs: number }> => (await apiClient.post('/ai/image_aesthetics/cancel', {})).data,
  cleanImageAesthetics: async (): Promise<{ message: string; queuedJobs: number }> => (await apiClient.post('/ai/image_aesthetics/clean', {})).data,
  startFaces: async (): Promise<{ message: string; queuedJobs: number }> => (await apiClient.post('/ai/faces/start', {})).data,
  cancelFaces: async (): Promise<{ message: string; queuedJobs: number }> => (await apiClient.post('/ai/faces/cancel', {})).data,
  cleanFaces: async (): Promise<{ message: string; queuedJobs: number }> => (await apiClient.post('/ai/faces/clean', {})).data,
  getOcrStatus: async (): Promise<AiTaskStatus> => (await apiClient.post('/ai/ocr/status', {})).data,
  getImageTaggingStatus: async (): Promise<AiTaskStatus> => (await apiClient.post('/ai/image_tagging/status', {})).data,
  getScreenshotDetectionStatus: async (): Promise<AiTaskStatus> => (await apiClient.post('/ai/screenshot_detection/status', {})).data,
  getDocumentDetectionStatus: async (): Promise<AiTaskStatus> => (await apiClient.post('/ai/document_detection/status', {})).data,
  getImageAestheticsStatus: async (): Promise<AiTaskStatus> => (await apiClient.post('/ai/image_aesthetics/status', {})).data,
  getFacesStatus: async (): Promise<FaceTaskStatus> => (await apiClient.post('/ai/faces/status', {})).data,
}
