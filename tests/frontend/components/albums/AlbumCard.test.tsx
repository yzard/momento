import { cleanup, fireEvent, render, screen } from '@testing-library/react'
import { afterEach, describe, expect, it, vi } from 'vitest'

import AlbumCard from '../../../../src/frontend/components/albums/AlbumCard'

const album = {
  id: 1,
  name: 'Trip',
  description: null,
  coverMediaId: 10,
  thumbnailMediaIds: [10, 11, 12, 13],
  mediaCount: 4,
  createdAt: '2026-01-01T00:00:00Z',
}

describe('AlbumCard', () => {
  afterEach(cleanup)

  it('renders a square four-image collage with overlaid album details', () => {
    const onClick = vi.fn()
    render(
      <AlbumCard
        album={album}
        thumbnailUrls={['first-cover', 'second-cover', 'third-cover', 'fourth-cover']}
        onClick={onClick}
        onDelete={vi.fn()}
      />
    )

    const card = screen.getByRole('button', { name: 'Trip, 4 memories' })
    expect(card.className).toContain('aspect-square')
    expect(card.querySelectorAll('img')).toHaveLength(4)
    expect([...card.querySelectorAll('img')].map((image) => image.getAttribute('src'))).toEqual([
      'first-cover',
      'second-cover',
      'third-cover',
      'fourth-cover',
    ])
    expect(screen.getByText('4 memories')).toBeTruthy()
    fireEvent.click(card)
    expect(onClick).toHaveBeenCalledOnce()
  })

  it('uses a singular memory label', () => {
    render(
      <AlbumCard
        album={{ ...album, thumbnailMediaIds: [10], mediaCount: 1 }}
        thumbnailUrls={['only-cover']}
        onClick={vi.fn()}
        onDelete={vi.fn()}
      />
    )

    expect(screen.getByText('1 memory')).toBeTruthy()
  })
})
