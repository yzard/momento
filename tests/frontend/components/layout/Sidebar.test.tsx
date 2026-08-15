import { render, screen } from '@testing-library/react'
import { MemoryRouter } from '../../../../src/frontend/node_modules/react-router-dom'
import { describe, expect, it, vi } from 'vitest'

vi.mock('../../../../src/frontend/hooks/useAuth', () => ({ useAuth: () => ({ user: { role: 'user' } }) }))

import Sidebar from '../../../../src/frontend/components/layout/Sidebar'

describe('Sidebar', () => {
  it('places Faces immediately after Map and before Utility', () => {
    render(<MemoryRouter><Sidebar isCollapsed={false} isMobileOpen toggleCollapse={vi.fn()} onNavigate={vi.fn()} /></MemoryRouter>)

    const labels = Array.from(screen.getByRole('navigation').querySelectorAll('a')).map((link) => link.textContent)
    expect(labels.indexOf('Faces')).toBe(labels.indexOf('Map') + 1)
    expect(labels.indexOf('Utility')).toBe(labels.indexOf('Faces') + 1)
  })
})
