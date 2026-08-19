import { cleanup, render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

const mocks = vi.hoisted(() => ({
  getStatus: vi.fn(),
  generate: vi.fn(),
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
  })

  afterEach(cleanup)

  it('generates metadata and shows status metrics', async () => {
    render(<MetadataPanel />)

    const generateButton = await screen.findByRole('button', { name: 'Generate' })
    expect(screen.queryByRole('button', { name: 'Reset & Generate All' })).toBeNull()
    await userEvent.click(generateButton)

    expect(mocks.generate).toHaveBeenCalledOnce()
    expect(screen.getByText('8')).toBeTruthy()
  })

  it('shows an action error', async () => {
    mocks.generate.mockRejectedValue(new Error('unavailable'))
    render(<MetadataPanel />)

    await userEvent.click(await screen.findByRole('button', { name: 'Generate' }))

    expect((await screen.findByRole('alert')).textContent).toBe('Could not complete the metadata action.')
  })
})
