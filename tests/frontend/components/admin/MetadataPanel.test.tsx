import { cleanup, render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

const mocks = vi.hoisted(() => ({
  getStatus: vi.fn(),
  generate: vi.fn(),
  reset: vi.fn(),
}))

vi.mock('../../../../src/frontend/api/metadata', () => ({
  metadataApi: mocks,
}))

import MetadataPanel from '../../../../src/frontend/components/admin/MetadataPanel'

describe('MetadataPanel', () => {
  beforeEach(() => {
    for (const mock of Object.values(mocks)) mock.mockReset()
    mocks.getStatus.mockResolvedValue({ status: 'idle', queuedJobs: 2, processingJobs: 1, completedJobs: 8, failedJobs: 0, errors: [] })
    mocks.generate.mockResolvedValue({ message: 'queued', queuedJobs: 2 })
    mocks.reset.mockResolvedValue({ message: 'reset', queuedJobs: 10 })
  })

  afterEach(cleanup)

  it('generates metadata and shows status metrics', async () => {
    render(<MetadataPanel />)

    expect(screen.getByText(/AI jobs are queued by their administrator controls or configured schedules/)).toBeTruthy()
    await userEvent.click(await screen.findByText('Generate'))

    expect(mocks.generate).toHaveBeenCalledOnce()
    expect(screen.getByText('8')).toBeTruthy()
  })

  it('shows an action error', async () => {
    mocks.reset.mockRejectedValue(new Error('unavailable'))
    render(<MetadataPanel />)

    await userEvent.click(await screen.findByText('Reset & Generate All'))

    expect((await screen.findByRole('alert')).textContent).toBe('Could not complete the metadata action.')
  })
})
