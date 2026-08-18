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
  dateTaken: null,
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
