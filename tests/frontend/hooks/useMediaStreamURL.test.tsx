import { act, renderHook, waitFor } from '@testing-library/react'
import { beforeEach, describe, expect, it, vi } from 'vitest'

const mocks = vi.hoisted(() => ({ getFileStreamURL: vi.fn() }))

vi.mock('../../../src/frontend/api/media', () => ({
  mediaApi: { getFileStreamURL: mocks.getFileStreamURL },
}))

import { useMediaStreamURL } from '../../../src/frontend/hooks/useMediaStreamURL'

describe('useMediaStreamURL', () => {
  beforeEach(() => {
    mocks.getFileStreamURL.mockReset()
  })

  it('loads only when enabled and ignores stale media responses', async () => {
    let resolveFirst!: (url: string) => void
    mocks.getFileStreamURL
      .mockImplementationOnce(
        () =>
          new Promise<string>((resolve) => {
            resolveFirst = resolve
          })
      )
      .mockResolvedValueOnce('/stream/2')
    const view = renderHook(({ mediaId, enabled }) => useMediaStreamURL(mediaId, enabled), {
      initialProps: { mediaId: 1 as number | null, enabled: false },
    })

    expect(mocks.getFileStreamURL).not.toHaveBeenCalled()
    view.rerender({ mediaId: 1, enabled: true })
    await waitFor(() => expect(mocks.getFileStreamURL).toHaveBeenCalledWith(1))
    view.rerender({ mediaId: 2, enabled: true })
    await waitFor(() => expect(mocks.getFileStreamURL).toHaveBeenCalledWith(2))
    await waitFor(() => expect(view.result.current.streamURL).toBe('/stream/2'))

    await act(async () => resolveFirst('/stream/1'))
    expect(view.result.current.streamURL).toBe('/stream/2')
  })

  it('automatically refreshes a failed media element only once', async () => {
    mocks.getFileStreamURL
      .mockResolvedValueOnce('/stream/first')
      .mockResolvedValueOnce('/stream/refreshed')
    const view = renderHook(() => useMediaStreamURL(3, true))
    await waitFor(() => expect(view.result.current.streamURL).toBe('/stream/first'))

    act(() => view.result.current.retryStreamOnce())
    await waitFor(() => expect(view.result.current.streamURL).toBe('/stream/refreshed'))
    act(() => view.result.current.retryStreamOnce())

    expect(mocks.getFileStreamURL).toHaveBeenCalledTimes(2)
  })
})
