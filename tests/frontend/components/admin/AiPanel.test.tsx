import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { cleanup, render, screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

const mocks = vi.hoisted(() => ({ trigger: vi.fn(), cancel: vi.fn(), clean: vi.fn(), triggerOcr: vi.fn(), cancelOcr: vi.fn(), cleanOcr: vi.fn(), triggerImageTagging: vi.fn(), cancelImageTagging: vi.fn(), cleanImageTagging: vi.fn(), triggerImageAesthetics: vi.fn(), cancelImageAesthetics: vi.fn(), cleanImageAesthetics: vi.fn(), triggerImageClustering: vi.fn(), cancelImageClustering: vi.fn(), cleanImageClustering: vi.fn(), startFaces: vi.fn(), cancelFaces: vi.fn(), cleanFaces: vi.fn(), getOcrStatus: vi.fn(), getImageTaggingStatus: vi.fn(), getImageAestheticsStatus: vi.fn(), getFacesStatus: vi.fn(), status: vi.fn() }))

vi.mock('../../../../src/frontend/api/ai', () => ({ aiApi: { trigger: mocks.trigger, cancel: mocks.cancel, clean: mocks.clean, triggerOcr: mocks.triggerOcr, cancelOcr: mocks.cancelOcr, cleanOcr: mocks.cleanOcr, triggerImageTagging: mocks.triggerImageTagging, cancelImageTagging: mocks.cancelImageTagging, cleanImageTagging: mocks.cleanImageTagging, triggerImageAesthetics: mocks.triggerImageAesthetics, cancelImageAesthetics: mocks.cancelImageAesthetics, cleanImageAesthetics: mocks.cleanImageAesthetics, triggerImageClustering: mocks.triggerImageClustering, cancelImageClustering: mocks.cancelImageClustering, cleanImageClustering: mocks.cleanImageClustering, startFaces: mocks.startFaces, cancelFaces: mocks.cancelFaces, cleanFaces: mocks.cleanFaces, getOcrStatus: mocks.getOcrStatus, getImageTaggingStatus: mocks.getImageTaggingStatus, getImageAestheticsStatus: mocks.getImageAestheticsStatus, getFacesStatus: mocks.getFacesStatus } }))
vi.mock('../../../../src/frontend/api/deduplicate', () => ({ deduplicateApi: { status: mocks.status } }))

import AiPanel from '../../../../src/frontend/components/admin/AiPanel'

describe('AiPanel', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    mocks.getOcrStatus.mockResolvedValue({ status: 'idle', completedJobs: 12 })
    mocks.getImageTaggingStatus.mockResolvedValue({ status: 'idle', completedJobs: 9 })
    mocks.getImageAestheticsStatus.mockResolvedValue({ status: 'idle', completedJobs: 8 })
    mocks.status.mockResolvedValue({ status: 'idle', ensembledMedia: 30, candidateComparisons: 44, clustersCreated: 5 })
    mocks.getFacesStatus.mockResolvedValue({ status: 'idle', completedJobs: 7, faceGroups: 6 })
    mocks.trigger.mockResolvedValue({})
    mocks.cancel.mockResolvedValue({})
    mocks.clean.mockResolvedValue({})
    mocks.triggerOcr.mockResolvedValue({})
    mocks.triggerImageTagging.mockResolvedValue({})
    mocks.triggerImageAesthetics.mockResolvedValue({})
    mocks.triggerImageClustering.mockResolvedValue({})
    mocks.startFaces.mockResolvedValue({})
  })

  it('starts all and individual AI job types', async () => {
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } })
    render(<QueryClientProvider client={queryClient}><AiPanel /></QueryClientProvider>)
    await screen.findByText('12')
    const user = userEvent.setup()

    await user.click(screen.getByRole('button', { name: 'All AI Jobs' }))
    await waitFor(() => expect(mocks.trigger).toHaveBeenCalledOnce())
    await user.click(screen.getByRole('button', { name: 'OCR' }))
    await waitFor(() => expect(mocks.triggerOcr).toHaveBeenCalledOnce())
    await user.click(screen.getByRole('button', { name: 'Image Tagging' }))
    await waitFor(() => expect(mocks.triggerImageTagging).toHaveBeenCalledOnce())
    await user.click(screen.getByRole('button', { name: 'Image Aesthetics' }))
    await waitFor(() => expect(mocks.triggerImageAesthetics).toHaveBeenCalledOnce())
    await user.click(screen.getByRole('button', { name: 'Image Clustering (Duplicate)' }))
    await waitFor(() => expect(mocks.triggerImageClustering).toHaveBeenCalledOnce())
    await user.click(screen.getByRole('button', { name: 'Face Detection' }))
    await waitFor(() => expect(mocks.startFaces).toHaveBeenCalledOnce())
  })

  it('cancels active all AI jobs', async () => {
    mocks.getOcrStatus.mockResolvedValue({ status: 'queued', completedJobs: 12 })
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } })
    render(<QueryClientProvider client={queryClient}><AiPanel /></QueryClientProvider>)
    await screen.findByText('12')

    await userEvent.click(screen.getByRole('button', { name: 'All AI Jobs' }))

    await waitFor(() => expect(mocks.cancel).toHaveBeenCalledOnce())
  })

  it('starts face detection without triggering image clustering', async () => {
    let resolveFaceStart: ((response: { message: string; queuedJobs: number }) => void) | undefined
    mocks.startFaces.mockReturnValue(new Promise((resolve) => { resolveFaceStart = resolve }))
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } })
    render(<QueryClientProvider client={queryClient}><AiPanel /></QueryClientProvider>)
    await screen.findByText('12')

    await userEvent.click(screen.getByRole('button', { name: 'Face Detection' }))

    await waitFor(() => expect(mocks.startFaces).toHaveBeenCalledOnce())
    expect(mocks.triggerImageClustering).not.toHaveBeenCalled()
    expect(screen.getByRole('button', { name: 'Image Clustering (Duplicate)' }).hasAttribute('disabled')).toBe(false)
    resolveFaceStart?.({ message: 'queued', queuedJobs: 1 })
  })

  it('renders a cleanup action for every AI control row', async () => {
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } })
    render(<QueryClientProvider client={queryClient}><AiPanel /></QueryClientProvider>)
    await screen.findByText('12')

    expect(screen.getByRole('button', { name: 'Clean All AI Data' })).toBeTruthy()
    expect(screen.getByRole('button', { name: 'Clean OCR Data' })).toBeTruthy()
    expect(screen.getByRole('button', { name: 'Clean Image Tagging Data' })).toBeTruthy()
    expect(screen.getByRole('button', { name: 'Clean Image Aesthetics Data' })).toBeTruthy()
    expect(screen.getByRole('button', { name: 'Clean Image Clustering (Duplicate) Data' })).toBeTruthy()
    expect(screen.getByRole('button', { name: 'Clean Face Detection Data' })).toBeTruthy()
  })

  afterEach(cleanup)

  it('shows consolidated processing metrics', async () => {
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } })
    render(<QueryClientProvider client={queryClient}><AiPanel /></QueryClientProvider>)
    expect(await screen.findByText('12')).toBeTruthy()
    expect(screen.getByText('9')).toBeTruthy()
    expect(screen.getByText('8')).toBeTruthy()
    expect(screen.getByText('30')).toBeTruthy()
    expect(screen.getByText('44')).toBeTruthy()
    expect(screen.getByText('5')).toBeTruthy()
    expect(screen.getByText('6')).toBeTruthy()
    expect(screen.getByText('Processed OCR')).toBeTruthy()
    expect(screen.getByText('Image clustering embeddings')).toBeTruthy()
    expect(screen.getByText('Duplicate groups')).toBeTruthy()
    expect(screen.getByText('Face groups')).toBeTruthy()
  })

  it('refreshes all metric sources', async () => {
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } })
    render(<QueryClientProvider client={queryClient}><AiPanel /></QueryClientProvider>)
    await screen.findByText('12')
    const initialOcrCalls = mocks.getOcrStatus.mock.calls.length
    const initialTaggingCalls = mocks.getImageTaggingStatus.mock.calls.length
    const initialDeduplicationCalls = mocks.status.mock.calls.length
    const initialAestheticsCalls = mocks.getImageAestheticsStatus.mock.calls.length
    const initialFacesCalls = mocks.getFacesStatus.mock.calls.length
    await userEvent.click(screen.getByRole('button', { name: 'Refresh metrics' }))

    await waitFor(() => {
      expect(mocks.getOcrStatus.mock.calls.length).toBeGreaterThan(initialOcrCalls)
      expect(mocks.getImageTaggingStatus.mock.calls.length).toBeGreaterThan(initialTaggingCalls)
      expect(mocks.status.mock.calls.length).toBeGreaterThan(initialDeduplicationCalls)
      expect(mocks.getImageAestheticsStatus.mock.calls.length).toBeGreaterThan(initialAestheticsCalls)
      expect(mocks.getFacesStatus.mock.calls.length).toBeGreaterThan(initialFacesCalls)
    })
  })
})
