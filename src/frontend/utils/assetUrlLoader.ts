import { mediaApi } from '../api/media'
import { trashApi } from '../api/trash'

class AssetUrlLoader {
  constructor(private readonly resolveUrl: (mediaId: number) => string) {}

  async load(mediaId: number): Promise<string> {
    return this.resolveUrl(mediaId)
  }
}

export const thumbnailUrlLoader = new AssetUrlLoader((mediaId) =>
  mediaApi.getThumbnailURL(mediaId, 'normal')
)
export const tinyThumbnailUrlLoader = new AssetUrlLoader((mediaId) =>
  mediaApi.getThumbnailURL(mediaId, 'tiny')
)
export const trashThumbnailUrlLoader = new AssetUrlLoader((mediaId) =>
  trashApi.getThumbnailURL(mediaId)
)
