import { Calendar, Camera, MapPin, FileType, KeyRound, Smartphone } from 'lucide-react'
import type { Media } from '../../api/types'
import { cn } from '../../lib/utils'

interface MediaDetailsProps {
  media: Media
  className?: string
}

export function MediaDetails({ media, className = '' }: MediaDetailsProps) {
  const formatFileSize = (bytes: number | null) => {
    if (bytes === null) return null
    if (bytes === 0) return '0 B'

    const units = ['B', 'KB', 'MB', 'GB']
    const unitIndex = Math.min(Math.floor(Math.log(bytes) / Math.log(1024)), units.length - 1)
    const value = bytes / Math.pow(1024, unitIndex)
    return `${value.toFixed(unitIndex === 0 ? 0 : 1)} ${units[unitIndex]}`
  }

  const rawValue = (value: string | number | null) =>
    value === null || value === '' ? null : String(value)
  const formatGpsValue = (value: number | null) =>
    value === null ? null : Number(value.toFixed(5)).toString()
  const combineValues = (...values: Array<string | number | null>) =>
    values.map(rawValue).filter((value): value is string => value !== null).join(', ') || null
  const hasValidGps = media.gpsLatitude !== null && media.gpsLongitude !== null &&
    media.gpsLatitude !== 0 && media.gpsLongitude !== 0
  const deviceName = `${media.cameraMake ?? ''} ${media.cameraModel ?? ''}`.toLowerCase()
  const isPhone = ['iphone', 'samsung', 'pixel', 'android'].some(name => deviceName.includes(name))
  const gpsValue = hasValidGps
    ? [formatGpsValue(media.gpsLatitude), formatGpsValue(media.gpsLongitude), formatGpsValue(media.gpsAltitude)]
      .filter((value): value is string => value !== null)
      .join(', ')
    : null

  const details = [
    { icon: Calendar, label: 'Date Taken', value: rawValue(media.dateTaken) },
    { icon: isPhone ? Smartphone : Camera, label: 'Camera', value: combineValues(media.cameraMake, media.cameraModel) },
    { icon: Camera, label: 'Lens', value: combineValues(media.lensMake, media.lensModel) },
    { icon: Camera, label: 'ISO', value: rawValue(media.iso) },
    { icon: Camera, label: 'Exposure Time', value: rawValue(media.exposureTime) },
    { icon: Camera, label: 'F Number', value: rawValue(media.fNumber) },
    { icon: Camera, label: 'Focal Length', value: rawValue(media.focalLength) },
    { icon: Camera, label: 'Focal Length 35mm', value: rawValue(media.focalLength35mm) },
    { icon: FileType, label: 'Media Type', value: rawValue(media.mediaType) },
    { icon: FileType, label: 'MIME Type', value: rawValue(media.mimeType) },
    { icon: FileType, label: 'Dimensions', value: media.width !== null && media.height !== null
      ? `${rawValue(media.width)} × ${rawValue(media.height)}`
      : null },
    { icon: FileType, label: 'Duration Seconds', value: rawValue(media.durationSeconds) },
    { icon: FileType, label: 'File Size', value: formatFileSize(media.fileSize) },
    { icon: FileType, label: 'Video Codec', value: rawValue(media.videoCodec) },
    { icon: FileType, label: 'Original Filename', value: rawValue(media.originalFilename) },
    { icon: FileType, label: 'Stored Filename', value: rawValue(media.filename) },
    { icon: MapPin, label: 'GPS (Lat, Long, Alt)', value: gpsValue },
    { icon: MapPin, label: 'Location', value: combineValues(media.locationCity, media.locationState, media.locationCountry) },
    { icon: KeyRound, label: 'Keywords', value: rawValue(media.keywords) },
    { icon: KeyRound, label: 'Content Hash', value: rawValue(media.contentHash) },
    { icon: Calendar, label: 'Created At', value: rawValue(media.createdAt) },
  ].filter(item => item.value !== null)

  return (
    <div className={cn("backdrop-blur-xl bg-card/95 rounded-2xl p-6 text-foreground border border-border shadow-2xl", className)}>
      <div className="mb-6 pb-4 border-b border-border">
        <h3 className="font-semibold text-base text-foreground break-all leading-relaxed">
          {media.originalFilename}
        </h3>
        <p className="text-xs text-muted-foreground mt-1 uppercase tracking-wide">
          {media.mediaType}
        </p>
      </div>
      <div className="space-y-4">
        {details.map((item) => (
          <div key={item.label} className="flex items-start gap-3">
            <item.icon className="w-4 h-4 text-muted-foreground mt-0.5 flex-shrink-0" />
            <div className="flex-1 min-w-0">
              <span className="text-[10px] uppercase tracking-wider text-muted-foreground font-bold block mb-0.5">
                {item.label}
              </span>
              <div className="text-sm text-foreground/90 font-medium break-all">
                {item.value}
              </div>
            </div>
          </div>
        ))}
      </div>
    </div>
  )
}
