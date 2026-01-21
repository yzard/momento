import { mediaApi } from '../api/media'

class ThumbnailBatcher {
  private queue: Set<number> = new Set()
  private pending: Map<number, ((url: string | null) => void)[]> = new Map()
  private missing: Set<number> = new Set()
  private timeout: ReturnType<typeof setTimeout> | null = null
  private batchDelayMs = 100

  load(id: number): Promise<string | null> {
    if (this.missing.has(id)) return Promise.resolve(null)

    const cached = mediaApi.getCachedThumbnailUrl(id)
    if (cached) return Promise.resolve(cached)

    return new Promise((resolve) => {
      if (!this.pending.has(id)) {
        this.pending.set(id, [])
      }
      this.pending.get(id)!.push(resolve)
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
      const results = await mediaApi.getThumbnailBatch(idsToFetch)

      idsToFetch.forEach((id) => {
        const resolvers = this.pending.get(id)
        if (!resolvers) return

        const url = results.get(id)
        if (url) {
          resolvers.forEach((r) => r(url))
          this.pending.delete(id)
          return
        }

        this.missing.add(id)
        resolvers.forEach((r) => r(null))
        this.pending.delete(id)
      })
    } catch (error) {
      console.error('Batch thumbnail load failed', error)
      idsToFetch.forEach((id) => {
        const resolvers = this.pending.get(id)
        if (resolvers) {
          resolvers.forEach((r) => r(null))
          this.pending.delete(id)
        }
      })
    }
  }
}

class PreviewBatcher {
  private queue: Set<number> = new Set()
  private pending: Map<number, ((url: string | null) => void)[]> = new Map()
  private timeout: ReturnType<typeof setTimeout> | null = null
  private batchDelayMs = 100

  load(id: number): Promise<string | null> {
    return new Promise((resolve) => {
      if (!this.pending.has(id)) {
        this.pending.set(id, [])
      }
      this.pending.get(id)!.push(resolve)
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
      const results = await mediaApi.getPreviewBatch(idsToFetch)

      idsToFetch.forEach((id) => {
        const resolvers = this.pending.get(id)
        if (resolvers) {
          const url = results.get(id) ?? null
          resolvers.forEach((r) => r(url))
          this.pending.delete(id)
        }
      })
    } catch (error) {
      console.error('Batch preview load failed', error)
      idsToFetch.forEach((id) => {
        const resolvers = this.pending.get(id)
        if (resolvers) {
          resolvers.forEach((r) => r(null))
          this.pending.delete(id)
        }
      })
    }
  }
}

export const batchLoader = new ThumbnailBatcher()
export const previewBatchLoader = new PreviewBatcher()
