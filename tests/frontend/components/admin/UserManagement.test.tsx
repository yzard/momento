import { cleanup, render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

const mocks = vi.hoisted(() => ({ listUsers: vi.fn(), createUser: vi.fn(), updateUser: vi.fn(), deleteUser: vi.fn() }))

vi.mock('../../../../src/frontend/api/admin', () => ({ adminApi: mocks }))

import UserManagement from '../../../../src/frontend/components/admin/UserManagement'

describe('UserManagement', () => {
  beforeEach(() => {
    for (const mock of Object.values(mocks)) mock.mockReset()
    mocks.listUsers.mockResolvedValue([])
  })

  afterEach(cleanup)

  it('opens an accessible user dialog with labelled form controls', async () => {
    render(<UserManagement />)
    await userEvent.click(await screen.findByRole('button', { name: 'Add User' }))

    expect(screen.getByRole('dialog', { name: 'Add User' })).toBeTruthy()
    expect(screen.getByLabelText('Username')).toBeTruthy()
    expect(screen.getByLabelText('Email')).toBeTruthy()
    expect(screen.getByLabelText('Password')).toBeTruthy()
    expect(screen.getByLabelText('Role')).toBeTruthy()
  })

  it('closes the dialog with Escape', async () => {
    render(<UserManagement />)
    await userEvent.click(await screen.findByRole('button', { name: 'Add User' }))
    await userEvent.keyboard('{Escape}')

    expect(screen.queryByRole('dialog')).toBeNull()
  })
})
