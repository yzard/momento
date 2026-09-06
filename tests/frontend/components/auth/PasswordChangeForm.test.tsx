import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import PasswordChangeForm from '../../../../src/frontend/components/auth/PasswordChangeForm'

const mocks = vi.hoisted(() => ({ changePassword: vi.fn() }))

vi.mock('../../../../src/frontend/hooks/useAuth', () => ({
  useAuth: () => ({ user: { username: 'alice' }, changePassword: mocks.changePassword }),
}))

beforeEach(() => {
  mocks.changePassword.mockResolvedValue(undefined)
})
afterEach(() => {
  cleanup()
  vi.clearAllMocks()
})

describe('PasswordChangeForm', () => {
  it.each(['modal', 'settings'] as const)(
    'identifies the account and password roles in the %s form',
    (layout) => {
      const { container } = render(<PasswordChangeForm onComplete={vi.fn()} layout={layout} />)
      const username = container.querySelector<HTMLInputElement>('input[name="username"]')
      expect(username?.value).toBe('alice')
      expect(username?.autocomplete).toBe('username')
      for (const [label, name, autocomplete] of [
        ['Current Password', 'currentPassword', 'current-password'],
        ['New Password', 'newPassword', 'new-password'],
        ['Confirm New Password', 'confirmPassword', 'new-password'],
      ]) {
        const input = screen.getByLabelText(label)
        expect(input.getAttribute('name')).toBe(name)
        expect(input.getAttribute('autocomplete')).toBe(autocomplete)
      }
    }
  )

  it('submits autofilled DOM values without requiring input events', async () => {
    const onComplete = vi.fn()
    render(<PasswordChangeForm onComplete={onComplete} layout="modal" />)
    const current = screen.getByLabelText<HTMLInputElement>('Current Password')
    const password = screen.getByLabelText<HTMLInputElement>('New Password')
    const confirm = screen.getByLabelText<HTMLInputElement>('Confirm New Password')
    current.value = 'autofilled-current'
    password.value = 'generated-password'
    confirm.value = 'generated-password'
    fireEvent.submit(current.form!)
    await waitFor(() => expect(onComplete).toHaveBeenCalledOnce())
    expect(mocks.changePassword).toHaveBeenCalledWith('autofilled-current', 'generated-password')
    expect(password.value).toBe('')
    expect(confirm.value).toBe('')
  })

  it('rejects mismatched autofill without erasing values and accepts a correction', async () => {
    render(<PasswordChangeForm onComplete={vi.fn()} layout="settings" />)
    const current = screen.getByLabelText<HTMLInputElement>('Current Password')
    const password = screen.getByLabelText<HTMLInputElement>('New Password')
    const confirm = screen.getByLabelText<HTMLInputElement>('Confirm New Password')
    current.value = 'autofilled-current'
    password.value = 'generated-password'
    confirm.value = 'different-password'
    fireEvent.submit(current.form!)
    expect(screen.getByRole('alert').textContent).toContain('New passwords do not match')
    expect(mocks.changePassword).not.toHaveBeenCalled()
    expect(password.value).toBe('generated-password')
    confirm.value = password.value
    fireEvent.submit(current.form!)
    await waitFor(() =>
      expect(mocks.changePassword).toHaveBeenCalledWith('autofilled-current', 'generated-password')
    )
  })

  it('uses the session-ending password change operation', async () => {
    const user = userEvent.setup()
    const onComplete = vi.fn()
    render(<PasswordChangeForm onComplete={onComplete} layout="modal" />)

    await user.type(screen.getByLabelText('Current Password'), 'old-password')
    await user.type(screen.getByLabelText('New Password'), 'new-password')
    await user.type(screen.getByLabelText('Confirm New Password'), 'new-password')
    await user.click(screen.getByRole('button', { name: 'Update Password' }))

    await waitFor(() => {
      expect(mocks.changePassword).toHaveBeenCalledWith('old-password', 'new-password')
      expect(onComplete).toHaveBeenCalledOnce()
    })
  })
})
