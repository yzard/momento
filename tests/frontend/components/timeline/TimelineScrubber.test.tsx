import { fireEvent, render, screen } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'
import TimelineScrubber from '../../../../src/frontend/components/timeline/TimelineScrubber'

const MARKERS = [
  { label: '2026-01', anchorDate: '2026-01-01' },
  { label: '2026-02', anchorDate: '2026-02-01' },
  { label: '2025-12', anchorDate: '2025-12-01' },
]

describe('TimelineScrubber', () => {
  it('selects labelled markers and supports keyboard navigation', () => {
    const onMarkerSelect = vi.fn()
    render(
      <TimelineScrubber
        markers={MARKERS}
        activeMarkerIndex={0}
        onMarkerSelect={onMarkerSelect}
        onWheel={vi.fn()}
      />
    )

    fireEvent.click(screen.getByRole('button', { name: 'Jump to Feb 2026' }))
    expect(onMarkerSelect).toHaveBeenLastCalledWith(MARKERS[1])

    fireEvent.keyDown(screen.getByRole('scrollbar', { name: 'Timeline index' }), { key: 'End' })
    expect(onMarkerSelect).toHaveBeenLastCalledWith(MARKERS[2])
    expect(screen.getAllByText('2026')).toHaveLength(1)
    expect(screen.getByText('2025')).toBeTruthy()
  })
})
