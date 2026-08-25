import { mediaApi } from '../api/media'
import { trashApi } from '../api/trash'

type AssetBatchLoader = (mediaIds: number[]) => Promise<Map<number, string>>
type CachedAssetLoader = (mediaId: number) => string | null | undefined

class AssetBatcher {
  private queue: Set<number> = new Set()
  private pending: Map<number, ((url: string | null) => void)[]> = new Map()
  private timeout: ReturnType<typeof setTimeout> | null = null
  private batchDelayMs = 100
  private loadBatch: AssetBatchLoader
  private getCached: CachedAssetLoader

  constructor(loadBatch: AssetBatchLoader, getCached: CachedAssetLoader) {
    this.loadBatch = loadBatch
    this.getCached = getCached
  }

  load(id: number): Promise<string | null> {
    const cached = this.getCached(id)
    if (cached) return Promise.resolve(cached)

    return new Promise((resolve) => {
      const resolvers = this.pending.get(id)
      if (resolvers) {
        resolvers.push(resolve)
        return
      }

      this.pending.set(id, [resolve])
      this.queue.add(id)

      if (!this.timeout) {
        this.timeout = setTimeout(() => this.flush(), this.batchDelayMs)
      }
    })
  }

  private async flush() {
    const idsToFetch = Array.from(this.queue)
    this.queue.clear()
    this.timeout = null

    if (idsToFetch.length === 0) return

    try {
      const results = await this.loadBatch(idsToFetch)

      idsToFetch.forEach((id) => {
        const resolvers = this.pending.get(id)
        if (!resolvers) return

        const url = results.get(id)
        resolvers.forEach((resolve) => resolve(url ?? null))
        this.pending.delete(id)
      })
    } catch (error) {
      console.error('Batch thumbnail load failed', error)
      idsToFetch.forEach((id) => {
        const resolvers = this.pending.get(id)
        if (resolvers) {
          resolvers.forEach((resolve) => resolve(null))
          this.pending.delete(id)
        }
      })
    }
  }
}

export const batchLoader = new AssetBatcher(
  (mediaIds) => mediaApi.getThumbnailBatch(mediaIds, 'normal'),
  (mediaId) => mediaApi.getCachedThumbnailUrl(mediaId, 'normal'),
)
export const tinyBatchLoader = new AssetBatcher(
  (mediaIds) => mediaApi.getThumbnailBatch(mediaIds, 'tiny'),
  (mediaId) => mediaApi.getCachedThumbnailUrl(mediaId, 'tiny'),
)
export const trashBatchLoader = new AssetBatcher(
  (mediaIds) => trashApi.getThumbnailBatch(mediaIds, 'tiny'),
  (mediaId) => mediaApi.getCachedThumbnailUrl(mediaId, 'tiny'),
)
