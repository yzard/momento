import { cleanup, fireEvent, render, screen } from '@testing-library/react'
import { afterEach, describe, expect, it, vi } from 'vitest'

import MediaSelectionToolbar from '../../../../src/frontend/components/media/MediaSelectionToolbar'

describe('MediaSelectionToolbar', () => {
  afterEach(cleanup)

  it('disables media actions until at least one item is selected', () => {
    render(
      <MediaSelectionToolbar
        selectedCount={0}
        isProcessing={false}
        onClear={vi.fn()}
        onDone={vi.fn()}
        onAddToAlbum={vi.fn()}
        onRemoveFromAlbum={null}
        onMoveToTrash={vi.fn()}
      />
    )

    expect(
      (
        screen.getByRole('button', {
          name: 'Add to album',
        }) as HTMLButtonElement
      ).disabled
    ).toBe(true)
    expect(
      (
        screen.getByRole('button', {
          name: 'Move to Trash',
        }) as HTMLButtonElement
      ).disabled
    ).toBe(true)
  })

  it('exposes the actions for the current view and finishes selection', () => {
    const addToAlbum = vi.fn()
    const finishSelection = vi.fn()
    render(
      <MediaSelectionToolbar
        selectedCount={3}
        isProcessing={false}
        onClear={vi.fn()}
        onDone={finishSelection}
        onAddToAlbum={addToAlbum}
        onRemoveFromAlbum={null}
        onMoveToTrash={vi.fn()}
      />
    )

    fireEvent.click(screen.getByRole('button', { name: 'Add to album' }))
    fireEvent.click(screen.getByRole('button', { name: 'Finish selecting media' }))

    expect(addToAlbum).toHaveBeenCalledOnce()
    expect(finishSelection).toHaveBeenCalledOnce()
    expect(screen.queryByRole('button', { name: 'Remove from album' })).toBeNull()
  })
})
