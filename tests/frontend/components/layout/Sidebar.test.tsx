import { render, screen } from '@testing-library/react'
import { MemoryRouter } from '../../../../src/frontend/node_modules/react-router-dom'
import { describe, expect, it, vi } from 'vitest'

vi.mock('../../../../src/frontend/hooks/useAuth', () => ({ useAuth: () => ({ user: { role: 'user' } }) }))

import Sidebar from '../../../../src/frontend/components/layout/Sidebar'

describe('Sidebar', () => {
  it('places Places between Map and Faces', () => {
    render(<MemoryRouter><Sidebar isCollapsed={false} isMobileOpen toggleCollapse={vi.fn()} onNavigate={vi.fn()} /></MemoryRouter>)

    const labels = Array.from(screen.getByRole('navigation').querySelectorAll('a')).map((link) => link.textContent)
    expect(labels.indexOf('Places')).toBe(labels.indexOf('Map') + 1)
    expect(labels.indexOf('Faces')).toBe(labels.indexOf('Places') + 1)
    expect(labels.indexOf('Utility')).toBe(labels.indexOf('Faces') + 1)
    expect(screen.getByText('v1.0.0')).toBeTruthy()
    expect(screen.queryByText('Momento v1.0.0')).toBeNull()
  })
})
