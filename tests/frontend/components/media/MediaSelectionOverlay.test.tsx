import { render } from '@testing-library/react'
import { describe, expect, it } from 'vitest'
import MediaSelectionOverlay from '../../../../src/frontend/components/media/MediaSelectionOverlay'

describe('MediaSelectionOverlay', () => {
  it.each([
    [true, 'bg-primary/25'],
    [false, 'bg-black/10'],
  ])('renders selected=%s with the expected state style', (selected, expectedClassName) => {
    const { container } = render(<MediaSelectionOverlay selected={selected} />)

    expect(container.firstElementChild?.classList.contains(expectedClassName)).toBe(true)
  })
})
