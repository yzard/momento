import { cleanup, fireEvent, render, screen } from '@testing-library/react'
import { afterEach, describe, expect, it, vi } from 'vitest'

import AlbumCard from '../../../../src/frontend/components/albums/AlbumCard'

const album = {
  id: 1,
  name: 'Trip',
  description: null,
  coverMediaId: 10,
  mediaCount: 2,
  createdAt: '2026-01-01T00:00:00Z',
}

describe('AlbumCard', () => {
  afterEach(cleanup)

  it('renders the cover supplied by the parent without loading it itself', () => {
    const onClick = vi.fn()
    render(
      <AlbumCard
        album={album}
        coverUrl="data:image/jpeg;base64,Y292ZXI="
        onClick={onClick}
        onDelete={vi.fn()}
      />,
    )

    expect(screen.getByRole('img', { name: 'Trip' }).getAttribute('src')).toBe(
      'data:image/jpeg;base64,Y292ZXI=',
    )
    fireEvent.click(screen.getByText('Trip'))
    expect(onClick).toHaveBeenCalledOnce()
  })
})
