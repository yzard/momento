import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { cleanup, render, screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

const mocks = vi.hoisted(() => ({
  start: vi.fn(),
  cancel: vi.fn(),
  clean: vi.fn(),
  startOcr: vi.fn(),
  cancelOcr: vi.fn(),
  cleanOcr: vi.fn(),
  startImageTagging: vi.fn(),
  cancelImageTagging: vi.fn(),
  cleanImageTagging: vi.fn(),
  startScreenshotDetection: vi.fn(),
  cancelScreenshotDetection: vi.fn(),
  cleanScreenshotDetection: vi.fn(),
  startDocumentDetection: vi.fn(),
  cancelDocumentDetection: vi.fn(),
  cleanDocumentDetection: vi.fn(),
  startImageAesthetics: vi.fn(),
  cancelImageAesthetics: vi.fn(),
  cleanImageAesthetics: vi.fn(),
  startFaces: vi.fn(),
  cancelFaces: vi.fn(),
  cleanFaces: vi.fn(),
  getOcrStatus: vi.fn(),
  getImageTaggingStatus: vi.fn(),
  getScreenshotDetectionStatus: vi.fn(),
  getDocumentDetectionStatus: vi.fn(),
  getImageAestheticsStatus: vi.fn(),
  getFacesStatus: vi.fn(),
  status: vi.fn(),
  startDeduplicate: vi.fn(),
  cancelDeduplicate: vi.fn(),
  cleanDeduplicate: vi.fn(),
}))

vi.mock('../../../../src/frontend/api/ai', () => ({
  aiApi: {
    start: mocks.start,
    cancel: mocks.cancel,
    clean: mocks.clean,
    startOcr: mocks.startOcr,
    cancelOcr: mocks.cancelOcr,
    cleanOcr: mocks.cleanOcr,
    startImageTagging: mocks.startImageTagging,
    cancelImageTagging: mocks.cancelImageTagging,
    cleanImageTagging: mocks.cleanImageTagging,
    startScreenshotDetection: mocks.startScreenshotDetection,
    cancelScreenshotDetection: mocks.cancelScreenshotDetection,
    cleanScreenshotDetection: mocks.cleanScreenshotDetection,
    startDocumentDetection: mocks.startDocumentDetection,
    cancelDocumentDetection: mocks.cancelDocumentDetection,
    cleanDocumentDetection: mocks.cleanDocumentDetection,
    startImageAesthetics: mocks.startImageAesthetics,
    cancelImageAesthetics: mocks.cancelImageAesthetics,
    cleanImageAesthetics: mocks.cleanImageAesthetics,
    startFaces: mocks.startFaces,
    cancelFaces: mocks.cancelFaces,
    cleanFaces: mocks.cleanFaces,
    getOcrStatus: mocks.getOcrStatus,
    getImageTaggingStatus: mocks.getImageTaggingStatus,
    getScreenshotDetectionStatus: mocks.getScreenshotDetectionStatus,
    getDocumentDetectionStatus: mocks.getDocumentDetectionStatus,
    getImageAestheticsStatus: mocks.getImageAestheticsStatus,
    getFacesStatus: mocks.getFacesStatus,
  },
}))
vi.mock('../../../../src/frontend/api/deduplicate', () => ({ deduplicateApi: {
  status: mocks.status,
  start: mocks.startDeduplicate,
  cancel: mocks.cancelDeduplicate,
  clean: mocks.cleanDeduplicate,
} }))

import AiPanel from '../../../../src/frontend/components/admin/AiPanel'

describe('AiPanel', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    mocks.getOcrStatus.mockResolvedValue({ status: 'idle', completedJobs: 12 })
    mocks.getImageTaggingStatus.mockResolvedValue({ status: 'idle', completedJobs: 9 })
    mocks.getScreenshotDetectionStatus.mockResolvedValue({ status: 'idle', completedJobs: 11 })
    mocks.getDocumentDetectionStatus.mockResolvedValue({ status: 'idle', completedJobs: 10 })
    mocks.getImageAestheticsStatus.mockResolvedValue({ status: 'idle', completedJobs: 8 })
    mocks.status.mockResolvedValue({ status: 'idle', ensembledMedia: 30, candidateComparisons: 44, clustersCreated: 5 })
    mocks.getFacesStatus.mockResolvedValue({ status: 'idle', completedJobs: 7, faceGroups: 6 })
    mocks.start.mockResolvedValue({})
    mocks.cancel.mockResolvedValue({})
    mocks.clean.mockResolvedValue({})
    mocks.startOcr.mockResolvedValue({})
    mocks.startImageTagging.mockResolvedValue({})
    mocks.startScreenshotDetection.mockResolvedValue({})
    mocks.cancelScreenshotDetection.mockResolvedValue({})
    mocks.cleanScreenshotDetection.mockResolvedValue({})
    mocks.startDocumentDetection.mockResolvedValue({})
    mocks.cancelDocumentDetection.mockResolvedValue({})
    mocks.cleanDocumentDetection.mockResolvedValue({})
    mocks.startImageAesthetics.mockResolvedValue({})
    mocks.startDeduplicate.mockResolvedValue({})
    mocks.startFaces.mockResolvedValue({})
  })

  it('starts all and individual AI job types', async () => {
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } })
    render(<QueryClientProvider client={queryClient}><AiPanel /></QueryClientProvider>)
    await screen.findByText('12')
    const user = userEvent.setup()

    await user.click(screen.getByRole('button', { name: 'All AI Jobs' }))
    await waitFor(() => expect(mocks.start).toHaveBeenCalledOnce())
    await user.click(screen.getByRole('button', { name: 'OCR' }))
    await waitFor(() => expect(mocks.startOcr).toHaveBeenCalledOnce())
    await user.click(screen.getByRole('button', { name: 'Image Tagging' }))
    await waitFor(() => expect(mocks.startImageTagging).toHaveBeenCalledOnce())
    await user.click(screen.getByRole('button', { name: 'Screenshot Detection' }))
    await waitFor(() => expect(mocks.startScreenshotDetection).toHaveBeenCalledOnce())
    await user.click(screen.getByRole('button', { name: 'Document Detection' }))
    await waitFor(() => expect(mocks.startDocumentDetection).toHaveBeenCalledOnce())
    await user.click(screen.getByRole('button', { name: 'Image Aesthetics' }))
    await waitFor(() => expect(mocks.startImageAesthetics).toHaveBeenCalledOnce())
    await user.click(screen.getByRole('button', { name: 'Deduplicate' }))
    await waitFor(() => expect(mocks.startDeduplicate).toHaveBeenCalledOnce())
    await user.click(screen.getByRole('button', { name: 'Face Detection' }))
    await waitFor(() => expect(mocks.startFaces).toHaveBeenCalledOnce())
  })

  it('cancels active detection tasks', async () => {
    mocks.getScreenshotDetectionStatus.mockResolvedValue({ status: 'processing', completedJobs: 11 })
    mocks.getDocumentDetectionStatus.mockResolvedValue({ status: 'queued', completedJobs: 10 })
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } })
    render(<QueryClientProvider client={queryClient}><AiPanel /></QueryClientProvider>)
    await screen.findByText('12')

    await userEvent.click(screen.getByRole('button', { name: 'Screenshot Detection' }))
    await waitFor(() => expect(mocks.cancelScreenshotDetection).toHaveBeenCalledOnce())
    await userEvent.click(screen.getByRole('button', { name: 'Document Detection' }))
    await waitFor(() => expect(mocks.cancelDocumentDetection).toHaveBeenCalledOnce())
  })

  it('cancels active all AI jobs', async () => {
    mocks.getOcrStatus.mockResolvedValue({ status: 'queued', completedJobs: 12 })
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } })
    render(<QueryClientProvider client={queryClient}><AiPanel /></QueryClientProvider>)
    await screen.findByText('12')

    await userEvent.click(screen.getByRole('button', { name: 'All AI Jobs' }))

    await waitFor(() => expect(mocks.cancel).toHaveBeenCalledOnce())
  })

  it('starts face detection without starting deduplication', async () => {
    let resolveFaceStart: ((response: { message: string; queuedJobs: number }) => void) | undefined
    mocks.startFaces.mockReturnValue(new Promise((resolve) => { resolveFaceStart = resolve }))
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } })
    render(<QueryClientProvider client={queryClient}><AiPanel /></QueryClientProvider>)
    await screen.findByText('12')

    await userEvent.click(screen.getByRole('button', { name: 'Face Detection' }))

    await waitFor(() => expect(mocks.startFaces).toHaveBeenCalledOnce())
    expect(mocks.startDeduplicate).not.toHaveBeenCalled()
    expect(screen.getByRole('button', { name: 'Deduplicate' }).hasAttribute('disabled')).toBe(false)
    resolveFaceStart?.({ message: 'queued', queuedJobs: 1 })
  })

  it('renders a cleanup action for every AI control row', async () => {
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } })
    render(<QueryClientProvider client={queryClient}><AiPanel /></QueryClientProvider>)
    await screen.findByText('12')

    expect(screen.getByRole('button', { name: 'Clean All AI Data' })).toBeTruthy()
    expect(screen.getByRole('button', { name: 'Clean OCR Data' })).toBeTruthy()
    expect(screen.getByRole('button', { name: 'Clean Image Tagging Data' })).toBeTruthy()
    expect(screen.getByRole('button', { name: 'Clean Screenshot Detection Data' })).toBeTruthy()
    expect(screen.getByRole('button', { name: 'Clean Document Detection Data' })).toBeTruthy()
    expect(screen.getByRole('button', { name: 'Clean Image Aesthetics Data' })).toBeTruthy()
    expect(screen.getByRole('button', { name: 'Clean Deduplicate Data' })).toBeTruthy()
    expect(screen.getByRole('button', { name: 'Clean Face Detection Data' })).toBeTruthy()
  })

  it('cleans screenshot and document detection data', async () => {
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } })
    render(<QueryClientProvider client={queryClient}><AiPanel /></QueryClientProvider>)
    await screen.findByText('12')

    await userEvent.click(screen.getByRole('button', { name: 'Clean Screenshot Detection Data' }))
    await waitFor(() => expect(mocks.cleanScreenshotDetection).toHaveBeenCalledOnce())
    await userEvent.click(screen.getByRole('button', { name: 'Clean Document Detection Data' }))
    await waitFor(() => expect(mocks.cleanDocumentDetection).toHaveBeenCalledOnce())
  })

  afterEach(() => {
    cleanup()
    vi.useRealTimers()
    vi.restoreAllMocks()
  })

  it('shows consolidated processing metrics', async () => {
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } })
    render(<QueryClientProvider client={queryClient}><AiPanel /></QueryClientProvider>)
    expect(await screen.findByText('12')).toBeTruthy()
    expect(screen.getByText('9')).toBeTruthy()
    expect(screen.getByText('11')).toBeTruthy()
    expect(screen.getByText('10')).toBeTruthy()
    expect(screen.getByText('8')).toBeTruthy()
    expect(screen.getByText('30')).toBeTruthy()
    expect(screen.getByText('44')).toBeTruthy()
    expect(screen.getByText('5')).toBeTruthy()
    expect(screen.getByText('6')).toBeTruthy()
    expect(screen.getByText('Processed OCR')).toBeTruthy()
    expect(screen.getByText('Processed screenshot detection')).toBeTruthy()
    expect(screen.getByText('Processed document detection')).toBeTruthy()
    expect(screen.getByText('Image clustering embeddings')).toBeTruthy()
    expect(screen.getByText('Duplicate groups')).toBeTruthy()
    expect(screen.getByText('Face groups')).toBeTruthy()
  })

  it('refreshes all metric sources every second while focused', async () => {
    vi.useFakeTimers()
    vi.spyOn(document, 'hasFocus').mockReturnValue(true)
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } })
    render(<QueryClientProvider client={queryClient}><AiPanel /></QueryClientProvider>)
    await vi.advanceTimersByTimeAsync(0)
    expect(screen.getByText('12')).toBeTruthy()
    const initialOcrCalls = mocks.getOcrStatus.mock.calls.length
    const initialTaggingCalls = mocks.getImageTaggingStatus.mock.calls.length
    const initialScreenshotDetectionCalls = mocks.getScreenshotDetectionStatus.mock.calls.length
    const initialDocumentDetectionCalls = mocks.getDocumentDetectionStatus.mock.calls.length
    const initialDeduplicationCalls = mocks.status.mock.calls.length
    const initialAestheticsCalls = mocks.getImageAestheticsStatus.mock.calls.length
    const initialFacesCalls = mocks.getFacesStatus.mock.calls.length
    expect(screen.queryByRole('button', { name: 'Refresh metrics' })).toBeNull()
    await vi.advanceTimersByTimeAsync(1000)

    expect(mocks.getOcrStatus.mock.calls.length).toBeGreaterThan(initialOcrCalls)
    expect(mocks.getImageTaggingStatus.mock.calls.length).toBeGreaterThan(initialTaggingCalls)
    expect(mocks.getScreenshotDetectionStatus.mock.calls.length).toBeGreaterThan(initialScreenshotDetectionCalls)
    expect(mocks.getDocumentDetectionStatus.mock.calls.length).toBeGreaterThan(initialDocumentDetectionCalls)
    expect(mocks.status.mock.calls.length).toBeGreaterThan(initialDeduplicationCalls)
    expect(mocks.getImageAestheticsStatus.mock.calls.length).toBeGreaterThan(initialAestheticsCalls)
    expect(mocks.getFacesStatus.mock.calls.length).toBeGreaterThan(initialFacesCalls)
  })
})
