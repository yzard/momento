import Lightbox from './Lightbox'
import type { LightboxController } from '../../hooks/useLightbox'

export default function ManagedLightbox({ controller }: { controller: LightboxController }) {
  if (!controller.state) return null
  return (
    <Lightbox
      mediaIds={controller.state.mediaIds}
      currentIndex={controller.state.currentIndex}
      onClose={controller.close}
      onIndexChange={controller.setCurrentIndex}
    />
  )
}
