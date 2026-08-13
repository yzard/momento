import { cleanup, render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

const mocks = vi.hoisted(() => ({ getMedia: vi.fn(), lightbox: vi.fn() }))

vi.mock('../../../src/frontend/components/map/MapView', () => ({
  default: ({ onClusterClick }: { onClusterClick: (payload: { bounds: { north: number; south: number; east: number; west: number }; geohashPrefixes: string[]; representativeId: number }) => Promise<void> }) => <button type="button" onClick={() => void onClusterClick({ bounds: { north: 2, south: 1, east: 2, west: 1 }, geohashPrefixes: ['dr5'], representativeId: 2 })}>Open cluster</button>,
}))
vi.mock('../../../src/frontend/components/viewer/Lightbox', () => ({
  default: (props: { mediaIds: number[]; currentIndex: number }) => {
    mocks.lightbox(props)
    return <div>Lightbox</div>
  },
}))
vi.mock('../../../src/frontend/api/map', () => ({ mapApi: { getMedia: mocks.getMedia } }))

import MapPage from '../../../src/frontend/pages/Map'

describe('MapPage', () => {
  beforeEach(() => {
    mocks.getMedia.mockReset()
    mocks.lightbox.mockReset()
    mocks.getMedia.mockResolvedValue({ items: [{ id: 1 }, { id: 2 }, { id: 3 }] })
  })

  afterEach(cleanup)

  it('opens clustered media sequentially from the representative image', async () => {
    render(<MapPage />)
    await userEvent.click(screen.getByRole('button', { name: 'Open cluster' }))
    await screen.findByText('Lightbox')

    expect(mocks.getMedia).toHaveBeenCalledWith({ bounds: { north: 2, south: 1, east: 2, west: 1 }, geohashPrefixes: ['dr5'] })
    expect(mocks.lightbox).toHaveBeenLastCalledWith(expect.objectContaining({ mediaIds: [1, 2, 3], currentIndex: 1 }))
  })
})
