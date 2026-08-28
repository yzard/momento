import { describe, expect, it } from 'vitest'
import { mediaFormatBadge } from '../../../src/frontend/lib/mediaFormat'

describe('mediaFormatBadge', () => {
  it.each([
    ['image/jpeg', 'photo.unknown', 'image', 'JPG'],
    [null, 'photo.HEIF', 'image', 'HEIC'],
    ['image/tiff', 'scan.unknown', 'image', 'TIFF'],
    ['image/qoi', 'image.unknown', 'image', 'QOI'],
    [null, 'image.QOI', 'image', 'QOI'],
    [null, 'capture.nef', 'image', 'RAW'],
    ['video/quicktime', 'clip.unknown', 'video', 'MOV'],
    [null, 'clip.webm', 'video', 'WEBM'],
  ] as const)('detects %s / %s as %s', (mimeType, filename, mediaType, expectedBadge) => {
    expect(mediaFormatBadge(mimeType, filename, mediaType)).toBe(expectedBadge)
  })

  it('returns null when neither the MIME type nor extension is recognized', () => {
    expect(mediaFormatBadge('application/octet-stream', 'media.bin', 'image')).toBeNull()
  })
})
