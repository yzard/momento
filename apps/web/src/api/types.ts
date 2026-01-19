export interface Media {
  id: number
  filename: string
  originalFilename: string
  mediaType: 'image' | 'video'
  mimeType: string
  width: number | null
  height: number | null
  fileSize: number
  durationSeconds: number | null
  dateTaken: string | null
  gpsLatitude: number | null
  gpsLongitude: number | null
  cameraMake: string | null
  cameraModel: string | null
  iso: number | null
  exposureTime: string | null
  fNumber: number | null
  focalLength: number | null
  gpsAltitude: number | null
  locationState: string | null
  locationCountry: string | null
  keywords: string | null
  createdAt: string
}

export interface Album {
  id: number
  name: string
  description: string | null
  coverMediaId: number | null
  mediaCount: number
  createdAt: string
}

export interface Tag {
  id: number
  name: string
}

export interface ShareLink {
  id: number
  token: string
  mediaId: number | null
  albumId: number | null
  hasPassword: boolean
  expiresAt: string | null
  viewCount: number
  createdAt: string
}

export interface PaginatedResponse<T> {
  items: T[]
  nextCursor: string | null
  hasMore: boolean
}

export interface TimelineGroup {
  date: string
  media: Media[]
}
