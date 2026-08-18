import { cleanup, render, screen } from '@testing-library/react'
import { afterEach, describe, expect, it } from 'vitest'

import type { Media } from '../../../../src/frontend/api/types'
import { MediaDetails } from '../../../../src/frontend/components/viewer/MediaDetails'

const media: Media = {
  id: 1,
  filename: '1.jpg',
  originalFilename: 'photo.jpg',
  mediaType: 'image',
  mimeType: 'image/jpeg',
  width: 1200,
  height: 800,
  fileSize: 1024,
  durationSeconds: null,
  dateTaken: '2025-01-25T14:33:00',
  gpsLatitude: 40.7128,
  gpsLongitude: -74.006,
  cameraMake: null,
  cameraModel: null,
  lensMake: null,
  lensModel: null,
  iso: null,
  exposureTime: null,
  fNumber: null,
  focalLength: null,
  gpsAltitude: null,
  locationCity: 'New York',
  locationState: 'New York',
  locationCountry: 'United States',
  videoCodec: null,
  focalLength35mm: null,
  keywords: null,
  contentHash: null,
  createdAt: '2026-08-17T00:00:00Z',
}

afterEach(cleanup)

describe('MediaDetails', () => {
  it('shows browser-local date taken and does not repeat the original filename row', () => {
    render(<MediaDetails media={media} />)

    expect(screen.getByText('2:33PM, Jan 25th, 2025')).toBeTruthy()
    expect(screen.queryByText('Original Filename')).toBeNull()
    expect(screen.getByRole('heading', { name: 'photo.jpg' })).toBeTruthy()
  })

  it('uses the correct ordinal suffix for date taken', () => {
    const { rerender } = render(
      <MediaDetails media={{ ...media, dateTaken: '2025-01-01T14:33:00' }} />,
    )
    expect(screen.getByText('2:33PM, Jan 1st, 2025')).toBeTruthy()

    for (const [dateTaken, expectedDate] of [
      ['2025-01-02T14:33:00', '2:33PM, Jan 2nd, 2025'],
      ['2025-01-03T14:33:00', '2:33PM, Jan 3rd, 2025'],
      ['2025-01-11T14:33:00', '2:33PM, Jan 11th, 2025'],
    ] as const) {
      rerender(<MediaDetails media={{ ...media, dateTaken }} />)
      expect(screen.getByText(expectedDate)).toBeTruthy()
    }
  })

  it('shows reverse-geocoded fields as one location row above GPS', () => {
    render(<MediaDetails media={media} />)

    const locationLabel = screen.getByText('Location')
    const gpsLabel = screen.getByText('GPS (Lat, Long, Alt)')

    expect(screen.getByText('New York, New York, United States')).toBeTruthy()
    expect(locationLabel.compareDocumentPosition(gpsLabel) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy()
  })

  it('hides the location row when no reverse-geocoded fields are available', () => {
    render(
      <MediaDetails
        media={{
          ...media,
          locationCity: null,
          locationState: null,
          locationCountry: null,
        }}
      />,
    )

    expect(screen.queryByText('Location')).toBeNull()
  })
})
