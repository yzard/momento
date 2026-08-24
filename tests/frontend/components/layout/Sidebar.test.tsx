import { cleanup, render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { MemoryRouter } from '../../../../src/frontend/node_modules/react-router-dom'
import { afterEach, describe, expect, it, vi } from 'vitest'

const mocks = vi.hoisted(() => ({ logout: vi.fn() }))

vi.mock('../../../../src/frontend/hooks/useAuth', () => ({
  useAuth: () => ({ user: { username: 'alice', role: 'user' }, logout: mocks.logout }),
}))

import Sidebar from '../../../../src/frontend/components/layout/Sidebar'

describe('Sidebar', () => {
  afterEach(() => {
    cleanup()
    vi.clearAllMocks()
  })

  it('places Places between Map and Faces', () => {
    render(<MemoryRouter><Sidebar isCollapsed={false} isMobileOpen toggleCollapse={vi.fn()} onNavigate={vi.fn()} /></MemoryRouter>)

    const labels = Array.from(screen.getByRole('navigation').querySelectorAll('a')).map((link) => link.textContent)
    expect(labels.indexOf('Places')).toBe(labels.indexOf('Map') + 1)
    expect(labels.indexOf('Faces')).toBe(labels.indexOf('Places') + 1)
    expect(labels.indexOf('Utility')).toBe(labels.indexOf('Faces') + 1)
    expect(screen.getByText('v1.0.0')).toBeTruthy()
    expect(screen.queryByText('Momento v1.0.0')).toBeNull()
    const androidDownload = screen.getByRole('link', { name: 'Android' })
    expect(androidDownload.getAttribute('href')).toBe('/momento-android.apk')
    expect(androidDownload.getAttribute('download')).toBe('momento-android.apk')
    expect(screen.getByRole('link', { name: 'Open account settings' }).getAttribute('href')).toBe('/settings')
    expect(screen.getByText('alice')).toBeTruthy()
    expect(screen.queryByRole('link', { name: 'Admin' })).toBeNull()
  })

  it('shows logout only while expanded', async () => {
    const user = userEvent.setup()
    const { rerender } = render(
      <MemoryRouter>
        <Sidebar isCollapsed={false} isMobileOpen toggleCollapse={vi.fn()} onNavigate={vi.fn()} />
      </MemoryRouter>,
    )

    await user.click(screen.getByRole('button', { name: 'Logout' }))
    expect(mocks.logout).toHaveBeenCalledOnce()

    rerender(
      <MemoryRouter>
        <Sidebar isCollapsed isMobileOpen toggleCollapse={vi.fn()} onNavigate={vi.fn()} />
      </MemoryRouter>,
    )

    expect(screen.getByRole('link', { name: 'Open account settings' })).toBeTruthy()
    expect(screen.queryByRole('button', { name: 'Logout' })).toBeNull()
  })

  it('shows screenshot and document timeline children', () => {
    render(<MemoryRouter initialEntries={['/timeline/screenshots']}><Sidebar isCollapsed={false} isMobileOpen toggleCollapse={vi.fn()} onNavigate={vi.fn()} /></MemoryRouter>)

    expect(screen.getByRole('link', { name: 'Screenshot' }).getAttribute('href')).toBe('/timeline/screenshots')
    expect(screen.getByRole('link', { name: 'Document' }).getAttribute('href')).toBe('/timeline/documents')
    expect(screen.getByRole('link', { name: 'Photos' }).getAttribute('href')).toBe('/timeline/photos')
  })
})
