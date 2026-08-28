import { cleanup, render, screen } from '@testing-library/react'
import { afterEach, describe, expect, it } from 'vitest'

import MemoryCardOverlay from '../../../../src/frontend/components/common/MemoryCardOverlay'

describe('MemoryCardOverlay', () => {
  afterEach(cleanup)

  it('renders the title, optional subtitle, and count badge', () => {
    const { rerender } = render(
      <MemoryCardOverlay
        title="Paris"
        subtitle="Ile-de-France, France"
        badge="8 media"
        headingLevel="h2"
      />
    )

    expect(screen.getByRole('heading', { level: 2, name: 'Paris' })).toBeTruthy()
    expect(screen.getByText('Ile-de-France, France')).toBeTruthy()
    expect(screen.getByText('8 media')).toBeTruthy()

    rerender(
      <MemoryCardOverlay title="Trip" subtitle={null} badge="4 memories" headingLevel="h3" />
    )

    expect(screen.getByRole('heading', { level: 3, name: 'Trip' })).toBeTruthy()
    expect(screen.queryByText('Ile-de-France, France')).toBeNull()
  })
})
