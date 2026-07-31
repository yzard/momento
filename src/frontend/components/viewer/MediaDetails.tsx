import { Calendar, Camera, MapPin, FileType, Tag, Smartphone } from 'lucide-react'
import type { Media } from '../../api/types'
import { cn } from '../../lib/utils'

interface MediaDetailsProps {
  media: Media
  className?: string
}

export function MediaDetails({ media, className = '' }: MediaDetailsProps) {
  const formatFileSize = (bytes: number | null | undefined) => {
    if (bytes == null) return null
    if (bytes === 0) return '0 B'
    const k = 1024
    const sizes = ['B', 'KB', 'MB', 'GB']
    const i = Math.floor(Math.log(bytes) / Math.log(k))
    return `${parseFloat((bytes / Math.pow(k, i)).toFixed(1))} ${sizes[i]}`
  }

  const formatDate = (date: string | null) => {
    if (!date) return null
    const d = new Date(date)
    const dateStr = d.toLocaleDateString(undefined, {
      weekday: 'short',
      year: 'numeric',
      month: 'short',
      day: 'numeric',
    })
    const timeStr = d.toLocaleTimeString(undefined, {
      hour: '2-digit',
      minute: '2-digit',
    })
    const timezone = d.toLocaleTimeString(undefined, { timeZoneName: 'short' }).split(' ').pop()
    return `${dateStr}, ${timeStr} ${timezone}`
  }

  const formatDuration = (seconds: number | null) => {
    if (!seconds) return null
    const mins = Math.floor(seconds / 60)
    const secs = Math.floor(seconds % 60)
    return `${mins}:${secs.toString().padStart(2, '0')}`
  }

  const formatCoords = (lat: number | null, lng: number | null) => {
    if (lat === null || lng === null) return null
    return `${lat.toFixed(4)}, ${lng.toFixed(4)}`
  }

  const formatNumber = (value: number, maxFractionDigits = 3) => {
    return value.toLocaleString(undefined, { maximumFractionDigits: maxFractionDigits })
  }

  const formatCodec = (codec: string | null) => {
    if (!codec) return null
    const normalized = codec.toLowerCase()
    if (normalized === 'hevc' || normalized === 'h265') return 'HEVC'
    if (normalized === 'h264') return 'H.264'
    if (normalized === 'av1') return 'AV1'
    if (normalized === 'vp9') return 'VP9'
    return codec.toUpperCase()
  }

  const deviceName = [media.cameraMake, media.cameraModel].filter(Boolean).join(' ')
  const settingsDeviceName = media.cameraModel || deviceName
  const settingsParts = [
    media.focalLength ? `${formatNumber(media.focalLength)}mm` : null,
    media.fNumber ? `ƒ/${formatNumber(media.fNumber, 2)}` : null,
    media.focalLength35mm ? `${Math.round(media.focalLength35mm)}mm` : null
  ].filter(Boolean)
  const cameraSettings = settingsParts.length > 0
    ? (settingsDeviceName ? `${settingsDeviceName} ${settingsParts.join(', ')}` : settingsParts.join(', '))
    : null

  const isPhone = deviceName.toLowerCase().includes('iphone') ||
                  deviceName.toLowerCase().includes('samsung') ||
                  deviceName.toLowerCase().includes('pixel') ||
                  deviceName.toLowerCase().includes('android')


  const locationName = [media.locationCity, media.locationState, media.locationCountry]
    .filter(Boolean)
    .join(', ')
  const coords = formatCoords(media.gpsLatitude, media.gpsLongitude)
  const altitude = media.gpsAltitude ? `${Math.round(media.gpsAltitude)}m` : null
  
  const LocationValue = () => {
    const hoverText = [
      coords ? `Lat/Long: ${coords}` : null,
      altitude ? `Alt: ${altitude}` : null
    ].filter(Boolean).join('\n')

    const display = locationName || coords

    if (!display) return null

    return (
      <div 
        title={hoverText || undefined}
        className={cn(hoverText && locationName ? "cursor-help decoration-dotted underline underline-offset-4 decoration-muted-foreground/40" : "")}
      >
        {display}
      </div>
    )
  }

  const keywordsList = media.keywords 
    ? media.keywords.split(',').map(k => k.trim()).filter(Boolean)
    : []

  const details = [
    { icon: Calendar, label: 'Time', value: formatDate(media.dateTaken) },
    {
      icon: isPhone ? Smartphone : Camera,
      label: 'Camera',
      value: deviceName || null
    },
    {
      icon: Camera,
      label: 'Camera Settings',
      value: cameraSettings || null
    },
    {
      icon: FileType,
      label: 'Format',
      value: [
        formatDuration(media.durationSeconds),
        formatCodec(media.videoCodec) || media.mimeType,
        media.width && media.height ? `${media.width} × ${media.height}` : null,
        formatFileSize(media.fileSize ?? null),
      ].filter(Boolean).join(', ')
    },
    { icon: FileType, label: 'Original Name', value: media.originalFilename || null },
    { 
      icon: MapPin, 
      label: 'Location', 
      value: (locationName || coords) ? <LocationValue /> : null 
    },
    {
      icon: Tag,
      label: 'Keywords',
      value: keywordsList.length > 0 ? (
        <div className="flex flex-wrap gap-1.5 pt-1">
          {keywordsList.map(k => (
            <span key={k} className="inline-flex items-center px-1.5 py-0.5 rounded-md bg-secondary/50 text-secondary-foreground text-[10px] font-medium border border-border/50">
              {k}
            </span>
          ))}
        </div>
      ) : null
    }
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
