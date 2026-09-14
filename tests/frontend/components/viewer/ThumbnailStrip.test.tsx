import { cleanup, fireEvent, render, screen, within } from '@testing-library/react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { ThumbnailStrip } from '../../../../src/frontend/components/viewer/ThumbnailStrip'

describe('ThumbnailStrip', () => {
  let resize: () => void
  let width: number
  const disconnect = vi.fn()
  beforeEach(() => {
    width = 600
    vi.spyOn(HTMLElement.prototype, 'clientWidth', 'get').mockImplementation(() => width)
    vi.stubGlobal(
      'ResizeObserver',
      class {
        constructor(callback: () => void) {
          resize = callback
        }
        observe() {}
        disconnect = disconnect
      }
    )
  })
  afterEach(() => {
    cleanup()
    vi.restoreAllMocks()
    vi.unstubAllGlobals()
  })

  it('virtualizes a large group, loads tiny thumbnails and selects by global index', () => {
    const ids = Array.from({ length: 1500 }, (_, i) => i + 100)
    const select = vi.fn()
    const view = render(<ThumbnailStrip mediaIds={ids} currentIndex={0} onIndexChange={select} />)
    const strip = screen.getByRole('region', { name: 'Media thumbnails' })
    expect(within(strip).getAllByRole('button').length).toBeLessThanOrEqual(14)
    expect(strip.querySelector('img')?.getAttribute('src')).toBe('/api/v1/media/100/thumbnail/tiny')
    expect(screen.getByRole('button', { name: 'View media 1' }).getAttribute('aria-current')).toBe(
      'true'
    )
    fireEvent.scroll(strip, { target: { scrollLeft: 89400 } })
    expect(screen.queryByRole('button', { name: 'View media 1' })).toBeNull()
    fireEvent.click(screen.getByRole('button', { name: 'View media 1500' }))
    expect(select).toHaveBeenCalledWith(1499)
    expect(within(strip).getAllByRole('button').length).toBeLessThanOrEqual(14)
    view.rerender(<ThumbnailStrip mediaIds={ids} currentIndex={800} onIndexChange={select} />)
    expect(
      screen.getByRole('button', { name: 'View media 801' }).getAttribute('aria-current')
    ).toBe('true')
    view.unmount()
    expect(disconnect).toHaveBeenCalled()
  })

  it('recalculates the visible range for viewport resizing and handles an empty group', async () => {
    const { act } = await import('@testing-library/react')
    const ids = Array.from({ length: 2000 }, (_, i) => i + 1)
    const view = render(
      <ThumbnailStrip mediaIds={ids} currentIndex={1000} onIndexChange={vi.fn()} />
    )
    width = 240
    act(() => resize())
    expect(screen.getAllByRole('button').length).toBeLessThanOrEqual(9)
    expect(screen.getByRole('button', { name: 'View media 1001' })).toBeTruthy()
    view.rerender(<ThumbnailStrip mediaIds={[]} currentIndex={0} onIndexChange={vi.fn()} />)
    expect(screen.queryAllByRole('button')).toHaveLength(0)
  })
})
