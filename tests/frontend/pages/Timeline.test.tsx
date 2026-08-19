import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { cleanup, render, screen } from '@testing-library/react'
import { afterEach, describe, expect, it, vi } from 'vitest'

const mocks = vi.hoisted(() => ({ timelineView: vi.fn() }))

vi.mock('../../../src/frontend/components/timeline/TimelineView', () => ({
  default: (props: unknown) => {
    mocks.timelineView(props)
    return <div>Timeline content</div>
  },
}))

import Timeline from '../../../src/frontend/pages/Timeline'

function renderTimeline(classification: 'screenshot' | 'document') {
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } })
  render(
    <QueryClientProvider client={queryClient}>
      <Timeline mediaType="image" classification={classification} />
    </QueryClientProvider>,
  )
}

describe('Timeline classification UI', () => {
  afterEach(() => {
    cleanup()
    vi.clearAllMocks()
  })

  it.each([
    ['screenshot' as const, 'Screenshots', 'Search screenshots...'],
    ['document' as const, 'Documents', 'Search documents...'],
  ])('renders %s title and search UI', (classification, title, placeholder) => {
    renderTimeline(classification)

    expect(screen.getByRole('heading', { name: title })).toBeTruthy()
    expect(screen.getByPlaceholderText(placeholder)).toBeTruthy()
    expect(mocks.timelineView).toHaveBeenCalledWith(expect.objectContaining({ mediaType: 'image', classification }))
  })
})
