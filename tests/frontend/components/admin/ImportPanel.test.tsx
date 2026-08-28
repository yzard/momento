import { cleanup, render, screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

const mocks = vi.hoisted(() => ({
  getStatus: vi.fn(),
  triggerLocal: vi.fn(),
}))

vi.mock('../../../../src/frontend/api/import', () => ({
  importApi: mocks,
}))

import ImportPanel from '../../../../src/frontend/components/admin/ImportPanel'

describe('ImportPanel', () => {
  beforeEach(() => {
    mocks.getStatus.mockReset()
    mocks.triggerLocal.mockReset()
    mocks.getStatus.mockResolvedValue({
      status: 'idle',
      totalFiles: 0,
      processedFiles: 0,
      totalMedia: 5103,
      successfulImports: 0,
      failedImports: 0,
      startedAt: null,
      completedAt: null,
      errors: [],
    })
    mocks.triggerLocal.mockResolvedValue({ message: 'started' })
  })

  afterEach(cleanup)

  it('starts a local import with one compact action', async () => {
    render(<ImportPanel />)

    await userEvent.click(await screen.findByRole('button', { name: 'Import Media' }))

    await waitFor(() => expect(mocks.triggerLocal).toHaveBeenCalledOnce())
    expect(screen.queryByText('Import Photos')).toBeNull()
  })

  it('shows concise progress while an import is running', async () => {
    mocks.getStatus.mockResolvedValue({
      status: 'running',
      totalFiles: 10,
      processedFiles: 4,
      totalMedia: 5103,
      successfulImports: 3,
      failedImports: 1,
      startedAt: '2026-08-18T12:00:00Z',
      completedAt: null,
      errors: [],
    })
    render(<ImportPanel />)

    const importButton = await screen.findByRole('button', {
      name: 'Importing...',
    })
    expect((importButton as HTMLButtonElement).disabled).toBe(true)
    expect(screen.getByRole('progressbar').getAttribute('aria-valuenow')).toBe('40')
    expect(screen.getByText('Imported').parentElement?.textContent).toContain('3')
    expect(screen.getByText('Failed').parentElement?.textContent).toContain('1')
    expect(screen.getByText('Total Media').parentElement?.textContent).toContain('5103')
  })
})
