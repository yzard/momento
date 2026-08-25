export interface Media {
  id: number
  filename: string
  originalFilename: string
  mediaType: 'image' | 'video'
  mimeType: string
  width: number | null
  height: number | null
  fileSize: number | null
  durationSeconds: number | null
  dateTaken: string | null
  gpsLatitude: number | null
  gpsLongitude: number | null
  cameraMake: string | null
  cameraModel: string | null
  lensMake: string | null
  lensModel: string | null
  iso: number | null
  exposureTime: string | null
  fNumber: number | null
  focalLength: number | null
  gpsAltitude: number | null
  locationCity: string | null
  locationState: string | null
  locationCountry: string | null
  videoCodec: string | null
  focalLength35mm: number | null
  keywords: string | null
  contentHash: string | null
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

export interface TimelineGroup {
  date: string
  media: Media[]
}
