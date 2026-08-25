import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { act, renderHook } from '@testing-library/react'
import type { ReactNode } from 'react'
import { describe, expect, it, vi } from 'vitest'

const mocks = vi.hoisted(() => ({ deleteMedia: vi.fn() }))

vi.mock('../../../src/frontend/api/media', () => ({
  mediaApi: { delete: mocks.deleteMedia },
}))

import { useDeleteMedia } from '../../../src/frontend/hooks/useMedia'

describe('useDeleteMedia', () => {
  it('invalidates only active media surfaces after deletion', async () => {
    mocks.deleteMedia.mockResolvedValue(undefined)
    const queryClient = new QueryClient()
    const invalidateQueries = vi.spyOn(queryClient, 'invalidateQueries')
    const wrapper = ({ children }: { children: ReactNode }) => (
      <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
    )
    const { result } = renderHook(() => useDeleteMedia(), { wrapper })

    await act(() => result.current.mutateAsync([7]))

    expect(mocks.deleteMedia).toHaveBeenCalledWith([7])
    expect(invalidateQueries.mock.calls.map(([filters]) => filters?.queryKey)).toEqual([
      ['timeline'],
      ['trash'],
      ['deduplicate', 'groups'],
    ])
  })
})
