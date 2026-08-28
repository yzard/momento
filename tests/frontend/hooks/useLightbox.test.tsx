import { act, renderHook } from '@testing-library/react'
import { describe, expect, it } from 'vitest'

import { useLightbox } from '../../../src/frontend/hooks/useLightbox'

describe('useLightbox', () => {
  it('opens the selected media and bounds index changes', () => {
    const hook = renderHook(() => useLightbox())

    act(() => hook.result.current.open(8, [7, 8, 9]))
    expect(hook.result.current.state).toEqual({
      mediaIds: [7, 8, 9],
      currentIndex: 1,
    })

    act(() => hook.result.current.setCurrentIndex(99))
    expect(hook.result.current.state?.currentIndex).toBe(2)
    act(() => hook.result.current.close())
    expect(hook.result.current.state).toBeNull()
  })

  it('does not open an empty media list', () => {
    const hook = renderHook(() => useLightbox())
    act(() => hook.result.current.openAtIndex([], 0))
    expect(hook.result.current.state).toBeNull()
  })
})
