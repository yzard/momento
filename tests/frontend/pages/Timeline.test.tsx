import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { act, cleanup, fireEvent, render, screen } from '@testing-library/react'
import { MemoryRouter } from 'react-router-dom'
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
      <MemoryRouter>
        <Timeline mediaType="image" classification={classification} />
      </MemoryRouter>
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

  it.each([
    [null, null],
    ['image', null],
    ['video', null],
    ['image', 'screenshot'],
    ['image', 'document'],
  ] as const)('keeps the active timeline filters when searching %s %s', (mediaType, classification) => {
    vi.useFakeTimers()
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } })
    render(
      <QueryClientProvider client={queryClient}>
        <MemoryRouter>
          <Timeline mediaType={mediaType} classification={classification} />
        </MemoryRouter>
      </QueryClientProvider>,
    )

    fireEvent.change(screen.getByRole('searchbox', { name: 'Search media' }), { target: { value: ' receipt ' } })
    act(() => vi.advanceTimersByTime(250))

    expect(mocks.timelineView).toHaveBeenLastCalledWith(expect.objectContaining({
      search: 'receipt',
      mediaType,
      classification,
    }))
    vi.useRealTimers()
  })
})
