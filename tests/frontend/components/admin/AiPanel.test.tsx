import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { cleanup, render, screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

const mocks = vi.hoisted(() => ({
  start: vi.fn(),
  status: vi.fn(),
  cancel: vi.fn(),
  clean: vi.fn(),
  startFeature: vi.fn(),
  cancelFeature: vi.fn(),
  cleanFeature: vi.fn(),
}))

vi.mock('../../../../src/frontend/api/ai', () => ({
  aiApi: {
    start: mocks.start,
    status: mocks.status,
    cancel: mocks.cancel,
    clean: mocks.clean,
    startFeature: mocks.startFeature,
    cancelFeature: mocks.cancelFeature,
    cleanFeature: mocks.cleanFeature,
  },
}))

import AiPanel from '../../../../src/frontend/components/admin/AiPanel'

const emptyJobs = {
  queued: 0,
  submitting: 0,
  submitted: 0,
  completed: 0,
  failed: 0,
  cancelled: 0,
}

function task(taskName: string, completed: number, state = 'idle') {
  return {
    task: taskName,
    enabled: true,
    state,
    jobs: { ...emptyJobs, completed },
    errors: [],
  }
}

function statusFixture(overrides: Record<string, string> = {}) {
  return {
    tasks: [
      task('ocr', 12, overrides.ocr),
      task('image_tagging', 9, overrides.image_tagging),
      task('screenshot_detection', 11, overrides.screenshot_detection),
      task('document_detection', 10, overrides.document_detection),
      task('image_aesthetics', 8, overrides.image_aesthetics),
      task('face_detection', 7, overrides.face_detection),
    ],
    deduplicate: {
      status: overrides.deduplicate ?? 'idle',
      runId: null,
      trigger: null,
      scheduledFor: null,
      startedAt: null,
      completedAt: null,
      ensembledMedia: 30,
      processedMedia: 30,
      candidateComparisons: 44,
      clustersCreated: 5,
      error: null,
      jobs: { ...emptyJobs },
    },
    faceGroups: 6,
  }
}

function renderPanel() {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  })
  render(<QueryClientProvider client={queryClient}><AiPanel /></QueryClientProvider>)
}

describe('AiPanel', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    mocks.status.mockResolvedValue(statusFixture())
    const action = { action: 'start', results: [] }
    mocks.start.mockResolvedValue(action)
    mocks.cancel.mockResolvedValue({ ...action, action: 'cancel' })
    mocks.clean.mockResolvedValue({ ...action, action: 'clean' })
    mocks.startFeature.mockResolvedValue(action)
    mocks.cancelFeature.mockResolvedValue({ ...action, action: 'cancel' })
    mocks.cleanFeature.mockResolvedValue({ ...action, action: 'clean' })
  })

  afterEach(() => {
    cleanup()
    vi.useRealTimers()
    vi.restoreAllMocks()
  })

  it('renders every metric from one aggregate status response', async () => {
    renderPanel()

    expect(await screen.findByText('12')).toBeTruthy()
    for (const value of ['9', '11', '10', '8', '30', '44', '5', '7', '6']) {
      expect(screen.getByText(value)).toBeTruthy()
    }
    expect(mocks.status).toHaveBeenCalledOnce()
  })

  it('starts global and exact named feature controls independently', async () => {
    renderPanel()
    await screen.findByText('12')
    const user = userEvent.setup()

    await user.click(screen.getByRole('button', { name: 'All AI Jobs' }))
    await waitFor(() => expect(mocks.start).toHaveBeenCalledOnce())
    await user.click(screen.getByRole('button', { name: 'Deduplicate' }))
    await waitFor(() => expect(mocks.startFeature).toHaveBeenCalledWith('deduplicate'))
    await user.click(screen.getByRole('button', { name: 'Face Detection' }))
    await waitFor(() => expect(mocks.startFeature).toHaveBeenCalledWith('face_detection'))
  })

  it('cancels each independently active task', async () => {
    mocks.status.mockResolvedValue(statusFixture({
      screenshot_detection: 'submitted',
      document_detection: 'queued',
    }))
    renderPanel()
    await screen.findByText('12')

    await userEvent.click(screen.getByRole('button', { name: 'Screenshot Detection' }))
    await waitFor(() => expect(mocks.cancelFeature).toHaveBeenCalledWith('screenshot_detection'))
    await userEvent.click(screen.getByRole('button', { name: 'Document Detection' }))
    await waitFor(() => expect(mocks.cancelFeature).toHaveBeenCalledWith('document_detection'))
  })

  it('exposes cleanup for each feature and calls the generic contract', async () => {
    renderPanel()
    await screen.findByText('12')

    expect(screen.getByRole('button', { name: 'Clean All AI Data' })).toBeTruthy()
    expect(screen.getByRole('button', { name: 'Clean OCR Data' })).toBeTruthy()
    expect(screen.getByRole('button', { name: 'Clean Image Tagging Data' })).toBeTruthy()
    expect(screen.getByRole('button', { name: 'Clean Screenshot Detection Data' })).toBeTruthy()
    expect(screen.getByRole('button', { name: 'Clean Document Detection Data' })).toBeTruthy()
    expect(screen.getByRole('button', { name: 'Clean Image Aesthetics Data' })).toBeTruthy()
    expect(screen.getByRole('button', { name: 'Clean Deduplicate Data' })).toBeTruthy()
    expect(screen.getByRole('button', { name: 'Clean Face Detection Data' })).toBeTruthy()

    await userEvent.click(screen.getByRole('button', { name: 'Clean Face Detection Data' }))
    await waitFor(() => expect(mocks.cleanFeature).toHaveBeenCalledWith('face_detection'))
  })

  it('polls only the aggregate status endpoint', async () => {
    vi.useFakeTimers()
    vi.spyOn(document, 'hasFocus').mockReturnValue(true)
    renderPanel()

    await vi.advanceTimersByTimeAsync(0)
    expect(mocks.status).toHaveBeenCalledTimes(1)
    await vi.advanceTimersByTimeAsync(1000)
    expect(mocks.status).toHaveBeenCalledTimes(2)
  })
})
