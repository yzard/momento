import { cleanup, render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

const mocks = vi.hoisted(() => ({
  getRegenerationStatus: vi.fn(),
  regenerateMedia: vi.fn(),
  resetLibrary: vi.fn(),
  cancelRegeneration: vi.fn(),
  triggerQueuedLlmJobs: vi.fn(),
}))

vi.mock('../../../../src/frontend/api/admin', () => ({
  adminApi: mocks,
}))

import MetadataPanel from '../../../../src/frontend/components/admin/MetadataPanel'

describe('MetadataPanel', () => {
  beforeEach(() => {
    for (const mock of Object.values(mocks)) mock.mockReset()
    mocks.getRegenerationStatus.mockResolvedValue({
      status: 'idle', totalJobs: 0, completedJobs: 0, metadataJobs: 0, metadataCompleted: 0,
      thumbnailJobs: 0, thumbnailsCompleted: 0, mediaTextJobs: 0, mediaTextCompleted: 0,
      startedAt: null, completedAt: null, errors: [],
    })
    mocks.triggerQueuedLlmJobs.mockResolvedValue({ message: 'started', status: 'running' })
  })

  afterEach(cleanup)

  it('processes queued LLM jobs', async () => {
    render(<MetadataPanel />)

    await userEvent.click(await screen.findByText('Process queued LLM jobs'))

    expect(mocks.triggerQueuedLlmJobs).toHaveBeenCalledOnce()
  })

  it('shows an error when processing queued LLM jobs fails', async () => {
    mocks.triggerQueuedLlmJobs.mockRejectedValue(new Error('unavailable'))
    render(<MetadataPanel />)

    await userEvent.click(await screen.findByText('Process queued LLM jobs'))

    expect((await screen.findByRole('alert')).textContent).toBe('Failed to process queued LLM jobs.')
  })
})
