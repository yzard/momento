import { act, renderHook } from '@testing-library/react'
import { describe, expect, it } from 'vitest'

import { toggleSelectedMediaId, useMediaSelection } from '../../../src/frontend/hooks/useMediaSelection'

describe('useMediaSelection', () => {
  it('toggles media identifiers without mutating the previous set', () => {
    const selectedMediaIds = new Set([7])

    const withAnotherMedia = toggleSelectedMediaId(selectedMediaIds, 9)
    const withoutOriginalMedia = toggleSelectedMediaId(withAnotherMedia, 7)

    expect(Array.from(selectedMediaIds)).toEqual([7])
    expect(Array.from(withAnotherMedia)).toEqual([7, 9])
    expect(Array.from(withoutOriginalMedia)).toEqual([9])
  })

  it('starts, clears, and exits selection mode explicitly', () => {
    const { result } = renderHook(() => useMediaSelection())

    act(() => result.current.startSelection())
    act(() => result.current.toggleSelection(42))
    expect(result.current.selectionMode).toBe(true)
    expect(Array.from(result.current.selectedMediaIds)).toEqual([42])

    act(() => result.current.clearSelection())
    expect(result.current.selectionMode).toBe(true)
    expect(result.current.selectedMediaIds.size).toBe(0)

    act(() => result.current.toggleSelection(43))
    act(() => result.current.cancelSelection())
    expect(result.current.selectionMode).toBe(false)
    expect(result.current.selectedMediaIds.size).toBe(0)
  })
})
