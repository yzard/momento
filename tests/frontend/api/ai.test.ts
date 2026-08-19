import { beforeEach, describe, expect, it, vi } from 'vitest'

const { post } = vi.hoisted(() => ({ post: vi.fn() }))

vi.mock('../../../src/frontend/api/client', () => ({ apiClient: { post } }))

import { aiApi } from '../../../src/frontend/api/ai'

describe('aiApi image aesthetics', () => {
  beforeEach(() => {
    post.mockReset()
    post.mockResolvedValue({ data: { message: 'ok', queuedJobs: 0 } })
  })

  it('uses the image aesthetics control and status endpoints', async () => {
    await aiApi.triggerImageAesthetics()
    await aiApi.cancelImageAesthetics()
    await aiApi.cleanImageAesthetics()
    await aiApi.getImageAestheticsStatus()

    expect(post.mock.calls).toEqual([
      ['/ai/image_aesthetics/trigger', {}],
      ['/ai/image_aesthetics/cancel', {}],
      ['/ai/image_aesthetics/clean', {}],
      ['/ai/image_aesthetics/status', {}],
    ])
  })

  it('uses the screenshot detection control and status endpoints', async () => {
    await aiApi.triggerScreenshotDetection()
    await aiApi.cancelScreenshotDetection()
    await aiApi.cleanScreenshotDetection()
    await aiApi.getScreenshotDetectionStatus()

    expect(post.mock.calls).toEqual([
      ['/ai/screenshot_detection/trigger', {}],
      ['/ai/screenshot_detection/cancel', {}],
      ['/ai/screenshot_detection/clean', {}],
      ['/ai/screenshot_detection/status', {}],
    ])
  })

  it('uses the document detection control and status endpoints', async () => {
    await aiApi.triggerDocumentDetection()
    await aiApi.cancelDocumentDetection()
    await aiApi.cleanDocumentDetection()
    await aiApi.getDocumentDetectionStatus()

    expect(post.mock.calls).toEqual([
      ['/ai/document_detection/trigger', {}],
      ['/ai/document_detection/cancel', {}],
      ['/ai/document_detection/clean', {}],
      ['/ai/document_detection/status', {}],
    ])
  })
})
