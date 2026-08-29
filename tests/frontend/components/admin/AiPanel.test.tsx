import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { cleanup, render, screen, waitFor, within } from '@testing-library/react'
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
  updateSchedule: vi.fn(),
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
    updateSchedule: mocks.updateSchedule,
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
    schedules: [
      ['ocr', '0 2 * * *'],
      ['image_tagging', '0 3 * * *'],
      ['screenshot_detection', '0 4 * * *'],
      ['document_detection', '0 5 * * *'],
      ['image_aesthetics', '0 6 * * *'],
      ['deduplicate', '0 7 * * *'],
      ['face_detection', '0 8 * * *'],
    ].map(([feature, cronExpression]) => ({ feature, cronExpression })),
  }
}

function renderPanel() {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  })
  render(
    <QueryClientProvider client={queryClient}>
      <AiPanel />
    </QueryClientProvider>
  )
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
    mocks.updateSchedule.mockImplementation(async (feature: string, cronExpression: string) => ({
      feature,
      cronExpression,
    }))
  })

  afterEach(() => {
    cleanup()
    vi.useRealTimers()
    vi.restoreAllMocks()
  })

  it('renders one status-table row per feature with the five requested job states', async () => {
    const fixture = statusFixture()
    fixture.tasks[0].jobs = {
      queued: 1,
      submitting: 2,
      submitted: 3,
      failed: 4,
      completed: 12,
      cancelled: 5,
    }
    mocks.status.mockResolvedValue(fixture)
    renderPanel()

    await screen.findByRole('cell', { name: '12' })
    const table = screen.getByRole('table', { name: 'AI work status' })
    for (const column of ['Queued', 'Submitting', 'Submitted', 'Failed', 'Completed']) {
      expect(within(table).getByRole('columnheader', { name: column })).toBeTruthy()
    }
    const rows = within(table).getAllByRole('row')
    expect(rows).toHaveLength(8)
    expect(
      within(within(table).getByRole('row', { name: 'OCR 1 2 3 4 12' }))
        .getAllByRole('cell')
        .map((cell) => cell.textContent)
    ).toEqual(['1', '2', '3', '4', '12'])
    expect(mocks.status).toHaveBeenCalledOnce()
  })

  it('renders every feature as one cron control-table row with the requested columns', async () => {
    renderPanel()

    const table = await screen.findByRole('table', {
      name: 'AI feature controls',
    })
    for (const column of [
      'Feature',
      'Minute',
      'Hour',
      'Day',
      'Month',
      'Weekday',
      'Save',
      'Start / Cancel',
      'Clean',
    ]) {
      expect(within(table).getByRole('columnheader', { name: column })).toBeTruthy()
    }
    expect(within(table).getAllByRole('row')).toHaveLength(8)
    expect(within(table).getAllByRole('rowheader')).toHaveLength(7)
    for (const feature of [
      'OCR',
      'Image Tagging',
      'Screenshot Detection',
      'Document Detection',
      'Image Aesthetics',
      'Deduplicate',
      'Face Detection',
    ]) {
      expect(
        within(table).getByRole('rowheader', {
          name: new RegExp(`^${feature}`),
        })
      ).toBeTruthy()
    }
    expect(within(table).queryByText('All AI Jobs')).toBeNull()
  })

  it('places every durable AI failure in a selectable log below the control table', async () => {
    const fixture = statusFixture()
    fixture.tasks[0].errors = ['OCR image could not be decoded']
    fixture.tasks[1].errors = ['Tagging runtime unavailable']
    fixture.deduplicate.error = 'Deduplication run failed'
    mocks.status.mockResolvedValue(fixture)
    renderPanel()

    const controlTable = await screen.findByRole('table', { name: 'AI feature controls' })
    const failureLog = screen.getByLabelText('AI failure log') as HTMLTextAreaElement
    expect(
      controlTable.compareDocumentPosition(failureLog) & Node.DOCUMENT_POSITION_FOLLOWING
    ).toBeTruthy()
    await waitFor(() => {
      expect(failureLog.value).toContain('[OCR] OCR image could not be decoded')
      expect(failureLog.value).toContain('[Image Tagging] Tagging runtime unavailable')
      expect(failureLog.value).toContain('[Deduplicate] Deduplication run failed')
    })
  })

  it('starts global and exact named feature controls independently', async () => {
    renderPanel()
    await screen.findByRole('cell', { name: '12' })
    const user = userEvent.setup()

    const startAllButton = screen.getByRole('button', {
      name: 'Start All AI Jobs',
    })
    const startDeduplicateButton = screen.getByRole('button', {
      name: 'Start Deduplicate',
    })
    expect(startAllButton.textContent).toBe('Start all')
    expect(startDeduplicateButton.textContent).toBe('Start')

    await user.click(startAllButton)
    await waitFor(() => expect(mocks.start).toHaveBeenCalledOnce())
    await user.click(startDeduplicateButton)
    await waitFor(() => expect(mocks.startFeature).toHaveBeenCalledWith('deduplicate'))
    await user.click(screen.getByRole('button', { name: 'Start Face Detection' }))
    await waitFor(() => expect(mocks.startFeature).toHaveBeenCalledWith('face_detection'))
  })

  it('cancels each independently active task', async () => {
    mocks.status.mockResolvedValue(
      statusFixture({
        screenshot_detection: 'submitted',
        document_detection: 'queued',
      })
    )
    renderPanel()
    await screen.findByRole('cell', { name: '12' })

    await userEvent.click(screen.getByRole('button', { name: 'Cancel Screenshot Detection' }))
    await waitFor(() => expect(mocks.cancelFeature).toHaveBeenCalledWith('screenshot_detection'))
    await userEvent.click(screen.getByRole('button', { name: 'Cancel Document Detection' }))
    await waitFor(() => expect(mocks.cancelFeature).toHaveBeenCalledWith('document_detection'))
  })

  it('requires confirmation before cleaning a feature', async () => {
    renderPanel()
    await screen.findByRole('cell', { name: '12' })

    expect(screen.getByRole('button', { name: 'Clean All AI Data' }).textContent).toBe('Clean all')
    expect(screen.getByRole('button', { name: 'Clean OCR Data' }).textContent).toBe('Clean')
    expect(screen.getByRole('button', { name: 'Clean Image Tagging Data' })).toBeTruthy()
    expect(screen.getByRole('button', { name: 'Clean Screenshot Detection Data' })).toBeTruthy()
    expect(screen.getByRole('button', { name: 'Clean Document Detection Data' })).toBeTruthy()
    expect(screen.getByRole('button', { name: 'Clean Image Aesthetics Data' })).toBeTruthy()
    expect(screen.getByRole('button', { name: 'Clean Deduplicate Data' })).toBeTruthy()
    expect(screen.getByRole('button', { name: 'Clean Face Detection Data' })).toBeTruthy()

    await userEvent.click(screen.getByRole('button', { name: 'Clean Face Detection Data' }))
    expect(mocks.cleanFeature).not.toHaveBeenCalled()
    expect(screen.getByRole('alertdialog', { name: 'Clean Face Detection data?' })).toBeTruthy()
    await userEvent.click(screen.getByRole('button', { name: 'Clean data' }))
    await waitFor(() => expect(mocks.cleanFeature).toHaveBeenCalledWith('face_detection'))
  })

  it('allows a global cleanup confirmation to be cancelled without deleting data', async () => {
    renderPanel()
    await screen.findByRole('cell', { name: '12' })

    await userEvent.click(screen.getByRole('button', { name: 'Clean All AI Data' }))
    expect(screen.getByRole('alertdialog', { name: 'Clean all AI data?' })).toBeTruthy()
    await userEvent.click(screen.getByRole('button', { name: 'Cancel' }))

    expect(mocks.clean).not.toHaveBeenCalled()
    expect(screen.queryByRole('alertdialog')).toBeNull()
  })

  it('edits and saves each feature cron schedule', async () => {
    renderPanel()
    const minuteInput = await screen.findByRole('textbox', {
      name: 'OCR cron minute',
    })
    const hourInput = screen.getByRole('textbox', { name: 'OCR cron hour' })
    const dayInput = screen.getByRole('textbox', { name: 'OCR cron day' })
    const monthInput = screen.getByRole('textbox', { name: 'OCR cron month' })
    const weekdayInput = screen.getByRole('textbox', {
      name: 'OCR cron weekday',
    })
    const user = userEvent.setup()

    await waitFor(() => expect((minuteInput as HTMLInputElement).value).toBe('0'))
    expect((hourInput as HTMLInputElement).value).toBe('2')
    expect((dayInput as HTMLInputElement).value).toBe('*')
    expect((monthInput as HTMLInputElement).value).toBe('*')
    expect((weekdayInput as HTMLInputElement).value).toBe('*')
    await user.clear(minuteInput)
    await user.type(minuteInput, '15')
    await user.clear(hourInput)
    await user.type(hourInput, '1')
    await user.clear(weekdayInput)
    await user.type(weekdayInput, '1-5')
    await user.click(screen.getByRole('button', { name: 'Save OCR cron schedule' }))

    await waitFor(() => expect(mocks.updateSchedule).toHaveBeenCalledWith('ocr', '15 1 * * 1-5'))
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
