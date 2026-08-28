import { render } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'

vi.mock('../../../../src/frontend/components/viewer/Lightbox', () => ({
  default: ({ currentIndex }: { currentIndex: number }) => <div>Viewer {currentIndex}</div>,
}))

import ManagedLightbox from '../../../../src/frontend/components/viewer/ManagedLightbox'

describe('ManagedLightbox', () => {
  it('renders only for an open controller', () => {
    const controller = {
      state: null,
      open: vi.fn(),
      openAtIndex: vi.fn(),
      close: vi.fn(),
      setCurrentIndex: vi.fn(),
    }
    const view = render(<ManagedLightbox controller={controller} />)
    expect(view.queryByText(/Viewer/)).toBeNull()

    view.rerender(
      <ManagedLightbox
        controller={{
          ...controller,
          state: { mediaIds: [1, 2], currentIndex: 1 },
        }}
      />
    )
    expect(view.getByText('Viewer 1')).toBeTruthy()
  })
})
