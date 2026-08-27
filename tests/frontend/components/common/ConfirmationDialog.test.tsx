import { fireEvent, render, screen } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'

import ConfirmationDialog from '../../../../src/frontend/components/common/ConfirmationDialog'

describe('ConfirmationDialog', () => {
  it('confirms or cancels a destructive media action', () => {
    const confirm = vi.fn()
    const cancel = vi.fn()
    render(
      <ConfirmationDialog
        title="Move selected media?"
        description="You can restore it later."
        confirmLabel="Move to Trash"
        isProcessing={false}
        destructive
        onConfirm={confirm}
        onCancel={cancel}
      />,
    )

    expect(screen.getByRole('alertdialog').getAttribute('aria-modal')).toBe('true')
    fireEvent.click(screen.getByRole('button', { name: 'Move to Trash' }))
    fireEvent.keyDown(window, { key: 'Escape' })

    expect(confirm).toHaveBeenCalledOnce()
    expect(cancel).toHaveBeenCalledOnce()
  })
})
