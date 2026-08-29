import { cleanup, render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

const mocks = vi.hoisted(() => ({
  listUsers: vi.fn(),
  createUser: vi.fn(),
  updateUser: vi.fn(),
  deleteUser: vi.fn(),
}))

vi.mock('../../../../src/frontend/api/admin', () => ({ adminApi: mocks }))
vi.mock('../../../../src/frontend/hooks/useAuth', () => ({
  useAuth: () => ({ user: { id: 1, username: 'manager', role: 'admin' } }),
}))

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

  it('sends the user ID in the update request body', async () => {
    mocks.listUsers.mockResolvedValue([
      {
        id: 7,
        username: 'member',
        email: 'member@example.com',
        role: 'user',
        isReserved: false,
        mustChangePassword: false,
        isActive: true,
        createdAt: '2024-01-01T00:00:00Z',
      },
    ])
    mocks.updateUser.mockResolvedValue(undefined)
    render(<UserManagement />)

    await userEvent.click(await screen.findByRole('button', { name: 'Deactivate' }))

    expect(mocks.updateUser).toHaveBeenCalledWith({
      userId: 7,
      isActive: false,
    })
  })

  it('requires explicit confirmation before deleting a user', async () => {
    mocks.listUsers.mockResolvedValue([
      {
        id: 7,
        username: 'member',
        email: 'member@example.com',
        role: 'user',
        isReserved: false,
        mustChangePassword: false,
        isActive: true,
        createdAt: '2024-01-01T00:00:00Z',
      },
    ])
    mocks.deleteUser.mockResolvedValue(undefined)
    render(<UserManagement />)

    await userEvent.click(await screen.findByRole('button', { name: 'Delete' }))
    expect(mocks.deleteUser).not.toHaveBeenCalled()
    await userEvent.click(screen.getByRole('button', { name: 'Delete user' }))

    expect(mocks.deleteUser).toHaveBeenCalledWith(7)
  })

  it('renders one table row per user and protects the reserved admin actions', async () => {
    mocks.listUsers.mockResolvedValue([
      {
        id: 2,
        username: 'admin',
        email: 'admin@example.com',
        role: 'admin',
        isReserved: true,
        mustChangePassword: false,
        isActive: true,
        createdAt: '2024-01-01T00:00:00Z',
      },
      {
        id: 7,
        username: 'member',
        email: 'member@example.com',
        role: 'user',
        isReserved: false,
        mustChangePassword: false,
        isActive: true,
        createdAt: '2024-01-01T00:00:00Z',
      },
    ])
    render(<UserManagement />)

    const table = await screen.findByRole('table', { name: 'Managed users' })
    expect(table.querySelectorAll('tbody tr')).toHaveLength(2)
    expect(screen.getByText('Reserved')).toBeTruthy()
    const deactivateButtons = screen.getAllByRole('button', { name: 'Deactivate' })
    const deleteButtons = screen.getAllByRole('button', { name: 'Delete' })
    expect((deactivateButtons[0] as HTMLButtonElement).disabled).toBe(true)
    expect((deleteButtons[0] as HTMLButtonElement).disabled).toBe(true)
    expect((deactivateButtons[1] as HTMLButtonElement).disabled).toBe(false)
    expect((deleteButtons[1] as HTMLButtonElement).disabled).toBe(false)
  })

  it('updates an existing user role from the same table', async () => {
    mocks.listUsers.mockResolvedValue([
      {
        id: 7,
        username: 'member',
        email: 'member@example.com',
        role: 'user',
        isReserved: false,
        mustChangePassword: false,
        isActive: true,
        createdAt: '2024-01-01T00:00:00Z',
      },
    ])
    mocks.updateUser.mockResolvedValue(undefined)
    render(<UserManagement />)

    await userEvent.selectOptions(await screen.findByLabelText('Role for member'), 'admin')
    expect(mocks.updateUser).toHaveBeenCalledWith({ userId: 7, role: 'admin' })
  })
})
