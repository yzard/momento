import { act, renderHook, waitFor } from '@testing-library/react'
import { afterEach, describe, expect, it, vi } from 'vitest'

import { usePollingStatus } from '../../../src/frontend/hooks/usePollingStatus'

function deferred<T>() {
  let resolve!: (value: T) => void
  const promise = new Promise<T>((resolvePromise) => {
    resolve = resolvePromise
  })
  return { promise, resolve }
}

describe('usePollingStatus', () => {
  afterEach(() => vi.restoreAllMocks())

  it('ignores an older status response that resolves after a refresh', async () => {
    const older = deferred<{ value: string }>()
    const newer = deferred<{ value: string }>()
    const load = vi.fn()
      .mockReturnValueOnce(older.promise)
      .mockReturnValueOnce(newer.promise)
    const { result } = renderHook(() => usePollingStatus(load, 'failed', 60_000))
    await waitFor(() => expect(load).toHaveBeenCalledOnce())

    act(() => {
      void result.current.refresh()
    })
    await waitFor(() => expect(load).toHaveBeenCalledTimes(2))
    await act(async () => newer.resolve({ value: 'newer' }))
    expect(result.current.status?.value).toBe('newer')

    await act(async () => older.resolve({ value: 'older' }))
    expect(result.current.status?.value).toBe('newer')
  })
})
