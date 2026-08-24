import { cleanup, render, screen } from '@testing-library/react'
import { afterEach, describe, expect, it, vi } from 'vitest'

import Header from '../../../../src/frontend/components/layout/Header'

afterEach(() => {
  cleanup()
  vi.clearAllMocks()
})

describe('Header', () => {
  it('keeps only the mobile navigation control', () => {
    render(<Header onMenuClick={vi.fn()} />)

    expect(screen.getByRole('button', { name: 'Open navigation' })).toBeTruthy()
    expect(screen.queryByText(/Welcome back/)).toBeNull()
    expect(screen.queryByRole('button', { name: 'Notifications' })).toBeNull()
    expect(screen.queryByRole('button', { name: 'Open account settings' })).toBeNull()
    expect(screen.queryByRole('button', { name: 'Logout' })).toBeNull()
  })
})
