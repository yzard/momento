import { apiClient } from './client'

export type AiFeature =
  | 'ocr'
  | 'image_tagging'
  | 'image_aesthetics'
  | 'screenshot_detection'
  | 'document_detection'
  | 'face_detection'
  | 'deduplicate'

export interface AiFeatureActionResult {
  feature: AiFeature
  outcome: string
  affectedJobs: number
  error: string | null
}

export interface AiActionResponse {
  action: 'start' | 'cancel' | 'clean'
  results: AiFeatureActionResult[]
}

export interface AiJobCounts {
  queued: number
  submitting: number
  submitted: number
  completed: number
  failed: number
  cancelled: number
}

export interface AiTaskStatus {
  task: Exclude<AiFeature, 'deduplicate'>
  enabled: boolean
  state: string
  jobs: AiJobCounts
  errors: string[]
}

export interface DeduplicateStatus {
  status: string
  runId: number | null
  trigger: string | null
  scheduledFor: string | null
  startedAt: string | null
  completedAt: string | null
  ensembledMedia: number
  processedMedia: number
  candidateComparisons: number
  clustersCreated: number
  error: string | null
  jobs: AiJobCounts
}

export interface AiStatusResponse {
  tasks: AiTaskStatus[]
  deduplicate: DeduplicateStatus
  faceGroups: number
  schedules: AiFeatureSchedule[]
}

export interface AiFeatureSchedule {
  feature: AiFeature
  cronExpression: string
}

export const aiApi = {
  start: async (): Promise<AiActionResponse> => (await apiClient.post<AiActionResponse>('/ai/start')).data,
  status: async (): Promise<AiStatusResponse> => (await apiClient.post<AiStatusResponse>('/ai/status')).data,
  cancel: async (): Promise<AiActionResponse> => (await apiClient.post<AiActionResponse>('/ai/cancel')).data,
  clean: async (): Promise<AiActionResponse> => (await apiClient.post<AiActionResponse>('/ai/clean')).data,
  startFeature: async (feature: AiFeature): Promise<AiActionResponse> =>
    (await apiClient.post<AiActionResponse>(`/ai/${feature}/start`)).data,
  cancelFeature: async (feature: AiFeature): Promise<AiActionResponse> =>
    (await apiClient.post<AiActionResponse>(`/ai/${feature}/cancel`)).data,
  cleanFeature: async (feature: AiFeature): Promise<AiActionResponse> =>
    (await apiClient.post<AiActionResponse>(`/ai/${feature}/clean`)).data,
  updateSchedule: async (feature: AiFeature, cronExpression: string): Promise<AiFeatureSchedule> =>
    (await apiClient.post<AiFeatureSchedule>('/ai/schedule/update', { feature, cronExpression })).data,
}
