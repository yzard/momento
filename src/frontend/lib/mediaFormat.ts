interface MediaFormatRule {
  badge: string
  mimeTypeFragments: readonly string[]
  fileExtensions: readonly string[]
}

const VIDEO_FORMAT_RULES: readonly MediaFormatRule[] = [
  { badge: 'MP4', mimeTypeFragments: ['mp4'], fileExtensions: ['mp4'] },
  { badge: 'MOV', mimeTypeFragments: ['quicktime', 'mov'], fileExtensions: ['mov'] },
  { badge: 'WEBM', mimeTypeFragments: ['webm'], fileExtensions: ['webm'] },
  { badge: 'AVI', mimeTypeFragments: ['avi'], fileExtensions: ['avi'] },
  { badge: 'MKV', mimeTypeFragments: ['mkv'], fileExtensions: ['mkv'] },
]

const IMAGE_FORMAT_RULES: readonly MediaFormatRule[] = [
  { badge: 'JPG', mimeTypeFragments: ['jpeg', 'jpg'], fileExtensions: ['jpg', 'jpeg'] },
  { badge: 'PNG', mimeTypeFragments: ['png'], fileExtensions: ['png'] },
  { badge: 'GIF', mimeTypeFragments: ['gif'], fileExtensions: ['gif'] },
  { badge: 'WEBP', mimeTypeFragments: ['webp'], fileExtensions: ['webp'] },
  { badge: 'QOI', mimeTypeFragments: ['qoi'], fileExtensions: ['qoi'] },
  { badge: 'HEIC', mimeTypeFragments: ['heic', 'heif'], fileExtensions: ['heic', 'heif'] },
  { badge: 'TIFF', mimeTypeFragments: ['tiff'], fileExtensions: ['tiff', 'tif'] },
  { badge: 'BMP', mimeTypeFragments: ['bmp'], fileExtensions: ['bmp'] },
  { badge: 'APNG', mimeTypeFragments: ['apng'], fileExtensions: ['apng'] },
  {
    badge: 'RAW',
    mimeTypeFragments: ['dng', 'cr2', 'arw', 'nef'],
    fileExtensions: ['dng', 'cr2', 'arw', 'nef'],
  },
]

export function mediaFormatBadge(
  mimeType: string | null,
  filename: string,
  mediaType: 'image' | 'video'
): string | null {
  const normalizedMimeType = mimeType?.toLowerCase() ?? ''
  const fileExtension = filename.split('.').pop()?.toLowerCase() ?? ''
  const formatRules = mediaType === 'video' ? VIDEO_FORMAT_RULES : IMAGE_FORMAT_RULES
  const matchingRule = formatRules.find(
    (formatRule) =>
      formatRule.mimeTypeFragments.some((fragment) => normalizedMimeType.includes(fragment)) ||
      formatRule.fileExtensions.includes(fileExtension)
  )

  return matchingRule?.badge ?? null
}
