import { cleanup, render, screen, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

const mocks = vi.hoisted(() => ({
  getStatus: vi.fn(),
  generate: vi.fn(),
  cancel: vi.fn(),
  clean: vi.fn(),
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
      cancellingJobs: 0,
      completedJobs: 8,
      failedJobs: 0,
      errors: [],
    })
    mocks.generate.mockResolvedValue({ message: 'queued', affectedJobs: 2 })
    mocks.cancel.mockResolvedValue({ message: 'cancelled', affectedJobs: 3 })
    mocks.clean.mockResolvedValue({ message: 'cleaned', affectedJobs: 10 })
  })

  afterEach(cleanup)

  it('generates metadata and shows status metrics', async () => {
    render(<MetadataPanel />)

    const generateButton = await screen.findByRole('button', {
      name: 'Generate',
    })
    const statusGrid = screen.getByText('Queued').parentElement?.parentElement

    expect(screen.getByRole('button', { name: 'Clean Data' })).toBeTruthy()
    expect(
      statusGrid?.compareDocumentPosition(generateButton) & Node.DOCUMENT_POSITION_FOLLOWING
    ).toBeTruthy()
    await userEvent.click(generateButton)

    expect(mocks.generate).toHaveBeenCalledOnce()
    expect(screen.getByText('8')).toBeTruthy()
  })

  it('confirms data cleanup and renders the selectable failure log below both actions', async () => {
    mocks.getStatus.mockResolvedValue({
      status: 'failed',
      queuedJobs: 0,
      processingJobs: 0,
      cancellingJobs: 0,
      completedJobs: 8,
      failedJobs: 1,
      errors: ['thumbnail generation failed'],
    })
    render(<MetadataPanel />)

    const cleanButton = await screen.findByRole('button', { name: 'Clean Data' })
    const failureLog = screen.getByLabelText('Metadata failure log') as HTMLTextAreaElement
    expect(
      cleanButton.compareDocumentPosition(failureLog) & Node.DOCUMENT_POSITION_FOLLOWING
    ).toBeTruthy()
    expect(failureLog.value).toBe('thumbnail generation failed')
    await userEvent.click(cleanButton)
    expect(mocks.clean).not.toHaveBeenCalled()
    await userEvent.click(
      within(screen.getByRole('alertdialog')).getByRole('button', { name: 'Clean Data' })
    )
    expect(mocks.clean).toHaveBeenCalledOnce()
  })

  it('switches Generate to Cancel while queued or processing work is active', async () => {
    mocks.getStatus.mockResolvedValue({
      status: 'processing',
      queuedJobs: 2,
      processingJobs: 1,
      cancellingJobs: 0,
      completedJobs: 8,
      failedJobs: 0,
      errors: [],
    })
    render(<MetadataPanel />)

    const cancelButton = await screen.findByRole('button', { name: 'Cancel' })
    expect((screen.getByRole('button', { name: 'Clean Data' }) as HTMLButtonElement).disabled).toBe(
      true
    )
    await userEvent.click(cancelButton)

    expect(mocks.cancel).toHaveBeenCalledOnce()
    expect(mocks.generate).not.toHaveBeenCalled()
  })

  it('disables both actions while cancellation is settling', async () => {
    mocks.getStatus.mockResolvedValue({
      status: 'cancelling',
      queuedJobs: 0,
      processingJobs: 0,
      cancellingJobs: 1,
      completedJobs: 8,
      failedJobs: 0,
      errors: [],
    })
    render(<MetadataPanel />)

    const cancellingButton = await screen.findByRole('button', { name: 'Cancelling…' })
    const cleanButton = screen.getByRole('button', { name: 'Clean Data' })
    expect((cancellingButton as HTMLButtonElement).disabled).toBe(true)
    expect((cleanButton as HTMLButtonElement).disabled).toBe(true)
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
