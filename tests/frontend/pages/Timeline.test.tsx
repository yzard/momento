import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { act, cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/react'
import { MemoryRouter } from 'react-router-dom'
import { afterEach, describe, expect, it, vi } from 'vitest'

const mocks = vi.hoisted(() => ({
  timelineView: vi.fn(),
  deleteMedia: vi.fn(),
}))

vi.mock('../../../src/frontend/components/timeline/TimelineView', () => ({
  default: (props: { selection: { toggleSelection: (mediaId: number) => void } | null }) => {
    mocks.timelineView(props)
    return (
      <div>
        Timeline content
        {props.selection && (
          <button type="button" onClick={() => props.selection?.toggleSelection(42)}>Select mock media</button>
        )}
      </div>
    )
  },
}))

vi.mock('../../../src/frontend/components/albums/AddToAlbumModal', () => ({
  default: ({ mediaIds }: { mediaIds: number[] }) => <div>Album picker: {mediaIds.join(',')}</div>,
}))

vi.mock('../../../src/frontend/api/media', () => ({
  mediaApi: { delete: mocks.deleteMedia },
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

  it('selects timeline media and opens the batch album picker', () => {
    renderTimeline('screenshot')

    fireEvent.click(screen.getByRole('button', { name: 'Select' }))
    fireEvent.click(screen.getByRole('button', { name: 'Select mock media' }))
    fireEvent.click(screen.getByRole('button', { name: 'Add to album' }))

    expect(screen.getByText('Album picker: 42')).toBeTruthy()
  })

  it('confirms before moving selected timeline media to Trash', async () => {
    mocks.deleteMedia.mockResolvedValue(undefined)
    renderTimeline('document')

    fireEvent.click(screen.getByRole('button', { name: 'Select' }))
    fireEvent.click(screen.getByRole('button', { name: 'Select mock media' }))
    fireEvent.click(screen.getByRole('button', { name: 'Move to Trash' }))
    fireEvent.click(within(screen.getByRole('alertdialog')).getByRole('button', { name: 'Move to Trash' }))

    await waitFor(() => expect(mocks.deleteMedia).toHaveBeenCalledWith([42], expect.anything()))
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
