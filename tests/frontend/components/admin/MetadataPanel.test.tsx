import { cleanup, render, screen, within } from '@testing-library/react'
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
    mocks.getStatus.mockResolvedValue({
      status: 'idle',
      queuedJobs: 2,
      processingJobs: 1,
      completedJobs: 8,
      failedJobs: 0,
      errors: [],
    })
    mocks.generate.mockResolvedValue({ message: 'queued', queuedJobs: 2 })
    mocks.reset.mockResolvedValue({ message: 'reset', queuedJobs: 10 })
  })

  afterEach(cleanup)

  it('generates metadata and shows status metrics', async () => {
    render(<MetadataPanel />)

    const generateButton = await screen.findByRole('button', {
      name: 'Generate',
    })
    const statusGrid = screen.getByText('Queued').parentElement?.parentElement

    expect(screen.getByRole('button', { name: 'Reset & regenerate' })).toBeTruthy()
    expect(
      statusGrid?.compareDocumentPosition(generateButton) & Node.DOCUMENT_POSITION_FOLLOWING
    ).toBeTruthy()
    await userEvent.click(generateButton)

    expect(mocks.generate).toHaveBeenCalledOnce()
    expect(screen.getByText('8')).toBeTruthy()
  })

  it('confirms reset and renders the selectable failure log below both actions', async () => {
    mocks.getStatus.mockResolvedValue({
      status: 'failed',
      queuedJobs: 0,
      processingJobs: 0,
      completedJobs: 8,
      failedJobs: 1,
      errors: ['thumbnail generation failed'],
    })
    render(<MetadataPanel />)

    const resetButton = await screen.findByRole('button', { name: 'Reset & regenerate' })
    const failureLog = screen.getByLabelText('Metadata failure log') as HTMLTextAreaElement
    expect(
      resetButton.compareDocumentPosition(failureLog) & Node.DOCUMENT_POSITION_FOLLOWING
    ).toBeTruthy()
    expect(failureLog.value).toBe('thumbnail generation failed')
    await userEvent.click(resetButton)
    expect(mocks.reset).not.toHaveBeenCalled()
    await userEvent.click(
      within(screen.getByRole('alertdialog')).getByRole('button', { name: 'Reset & regenerate' })
    )
    expect(mocks.reset).toHaveBeenCalledOnce()
  })

  it('shows an action error', async () => {
    mocks.generate.mockRejectedValue(new Error('unavailable'))
    render(<MetadataPanel />)

    await userEvent.click(await screen.findByRole('button', { name: 'Generate' }))

    expect((await screen.findByRole('alert')).textContent).toBe(
      'Could not complete the metadata action.'
    )
  })
})
