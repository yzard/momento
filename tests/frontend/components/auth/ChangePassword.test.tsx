import { render, screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { beforeEach, describe, expect, it, vi } from 'vitest'

import ChangePassword from '../../../../src/frontend/components/auth/ChangePassword'

const mocks = vi.hoisted(() => ({ changePassword: vi.fn() }))

vi.mock('../../../../src/frontend/hooks/useAuth', () => ({
  useAuth: () => ({ changePassword: mocks.changePassword }),
}))

beforeEach(() => mocks.changePassword.mockResolvedValue(undefined))

describe('ChangePassword', () => {
  it('uses the session-ending password change operation', async () => {
    const user = userEvent.setup()
    const onComplete = vi.fn()
    render(<ChangePassword onComplete={onComplete} />)

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
