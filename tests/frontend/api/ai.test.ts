import { beforeEach, describe, expect, it, vi } from 'vitest'

const { post } = vi.hoisted(() => ({ post: vi.fn() }))

vi.mock('../../../src/frontend/api/client', () => ({ apiClient: { post } }))

import { aiApi } from '../../../src/frontend/api/ai'

describe('aiApi', () => {
  beforeEach(() => {
    post.mockReset()
    post.mockResolvedValue({ data: { action: 'start', results: [] } })
  })

  it('uses bodyless aggregate control and status endpoints', async () => {
    await aiApi.start()
    await aiApi.status()
    await aiApi.cancel()
    await aiApi.clean()

    expect(post.mock.calls).toEqual([['/ai/start'], ['/ai/status'], ['/ai/cancel'], ['/ai/clean']])
  })

  it('uses exact task identifiers for feature controls', async () => {
    await aiApi.startFeature('face_detection')
    await aiApi.cancelFeature('image_aesthetics')
    await aiApi.cleanFeature('deduplicate')

    expect(post.mock.calls).toEqual([
      ['/ai/face_detection/start'],
      ['/ai/image_aesthetics/cancel'],
      ['/ai/deduplicate/clean'],
    ])
  })

  it('updates one exact feature schedule with its five-field cron expression', async () => {
    await aiApi.updateSchedule('ocr', '15 1 * * 1-5')

    expect(post).toHaveBeenCalledWith('/ai/schedule/update', {
      feature: 'ocr',
      cronExpression: '15 1 * * 1-5',
    })
  })
})
