import { cleanup, render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { MemoryRouter } from '../../../../src/frontend/node_modules/react-router-dom'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

const mocks = vi.hoisted(() => ({
  logout: vi.fn(),
  user: { username: 'alice', role: 'user' as 'admin' | 'user' },
}))

vi.mock('../../../../src/frontend/hooks/useAuth', () => ({
  useAuth: () => ({
    user: mocks.user,
    logout: mocks.logout,
  }),
}))

import Sidebar from '../../../../src/frontend/components/layout/Sidebar'

describe('Sidebar', () => {
  afterEach(() => {
    cleanup()
    vi.clearAllMocks()
  })

  beforeEach(() => {
    mocks.user.role = 'user'
  })

  it('places Places between Map and Faces', () => {
    render(
      <MemoryRouter>
        <Sidebar isCollapsed={false} isMobileOpen toggleCollapse={vi.fn()} onNavigate={vi.fn()} />
      </MemoryRouter>
    )

    const labels = Array.from(screen.getByRole('navigation').querySelectorAll('a')).map(
      (link) => link.textContent
    )
    expect(labels.indexOf('Places')).toBe(labels.indexOf('Map') + 1)
    expect(labels.indexOf('Faces')).toBe(labels.indexOf('Places') + 1)
    expect(labels.indexOf('Utility')).toBe(labels.indexOf('Faces') + 1)
    expect(screen.getByText('v1.0.0')).toBeTruthy()
    expect(screen.queryByText('Momento v1.0.0')).toBeNull()
    const androidDownload = screen.getByRole('link', {
      name: 'Download Android app',
    })
    expect(androidDownload.getAttribute('href')).toBe('/momento-android.apk')
    expect(androidDownload.getAttribute('download')).toBe('momento-android.apk')
    expect(screen.getByRole('link', { name: 'Open account settings' }).getAttribute('href')).toBe(
      '/settings'
    )
    expect(screen.getByText('alice')).toBeTruthy()
    expect(screen.queryByRole('link', { name: 'Admin' })).toBeNull()
  })

  it('shows only accessible navigation icons and no labels while collapsed', async () => {
    const user = userEvent.setup()
    const { rerender } = render(
      <MemoryRouter>
        <Sidebar isCollapsed={false} isMobileOpen toggleCollapse={vi.fn()} onNavigate={vi.fn()} />
      </MemoryRouter>
    )

    await user.click(screen.getByRole('button', { name: 'Logout' }))
    expect(mocks.logout).toHaveBeenCalledOnce()

    rerender(
      <MemoryRouter>
        <Sidebar isCollapsed isMobileOpen toggleCollapse={vi.fn()} onNavigate={vi.fn()} />
      </MemoryRouter>
    )

    expect(screen.getByRole('link', { name: 'Open account settings' })).toBeTruthy()
    expect(screen.queryByRole('button', { name: 'Logout' })).toBeNull()
    expect(screen.getByRole('link', { name: 'Download Android app' })).toBeTruthy()
    for (const label of ['Timeline', 'Albums', 'Map', 'Places', 'Faces', 'Utility', 'Trash']) {
      expect(screen.getByRole('link', { name: label })).toBeTruthy()
      expect(screen.queryByText(label)).toBeNull()
    }
  })

  it('shows the expandable Admin menu below Trash only for administrators', async () => {
    mocks.user.role = 'admin'
    render(
      <MemoryRouter initialEntries={['/settings']}>
        <Sidebar isCollapsed={false} isMobileOpen toggleCollapse={vi.fn()} onNavigate={vi.fn()} />
      </MemoryRouter>
    )

    const trashLink = screen.getByRole('link', { name: 'Trash' })
    const adminLink = screen.getByRole('link', { name: 'Admin' })
    const navigation = screen.getByRole('navigation')
    const adminAnchor = navigation.querySelector('[data-navigation-anchor="bottom"]')
    expect(
      trashLink.compareDocumentPosition(adminLink) & Node.DOCUMENT_POSITION_FOLLOWING
    ).toBeTruthy()
    expect(navigation.className).toContain('flex-col')
    expect(adminAnchor?.className).toContain('mt-auto')
    expect(adminAnchor).toBe(navigation.lastElementChild)
    expect(navigation.nextElementSibling?.className).toContain('border-t')
    expect(
      adminLink.compareDocumentPosition(
        screen.getByRole('link', { name: 'Open account settings' })
      ) & Node.DOCUMENT_POSITION_FOLLOWING
    ).toBeTruthy()
    await userEvent.click(screen.getByRole('button', { name: 'Expand Admin' }))
    expect(screen.getByRole('link', { name: 'Import' }).getAttribute('href')).toBe('/admin/import')
    expect(screen.getByRole('link', { name: 'Metadata' }).getAttribute('href')).toBe(
      '/admin/metadata'
    )
    expect(screen.getByRole('link', { name: 'AI' }).getAttribute('href')).toBe('/admin/ai')
    expect(screen.getByRole('link', { name: 'User Management' }).getAttribute('href')).toBe(
      '/admin/users'
    )
    expect(
      screen.getByRole('link', { name: 'Import' }).compareDocumentPosition(adminLink) &
        Node.DOCUMENT_POSITION_FOLLOWING
    ).toBeTruthy()
  })

  it('shows screenshot and document timeline children', () => {
    render(
      <MemoryRouter initialEntries={['/timeline/screenshots']}>
        <Sidebar isCollapsed={false} isMobileOpen toggleCollapse={vi.fn()} onNavigate={vi.fn()} />
      </MemoryRouter>
    )

    expect(screen.getByRole('link', { name: 'Screenshot' }).getAttribute('href')).toBe(
      '/timeline/screenshots'
    )
    expect(screen.getByRole('link', { name: 'Document' }).getAttribute('href')).toBe(
      '/timeline/documents'
    )
    expect(screen.getByRole('link', { name: 'Photos' }).getAttribute('href')).toBe(
      '/timeline/photos'
    )
  })

  it('always shows expandable menu controls and expands an unfocused section', async () => {
    const user = userEvent.setup()
    const { rerender } = render(
      <MemoryRouter initialEntries={['/albums']}>
        <Sidebar isCollapsed={false} isMobileOpen toggleCollapse={vi.fn()} onNavigate={vi.fn()} />
      </MemoryRouter>
    )

    const timelineControl = screen.getByRole('button', { name: 'Expand Timeline' })
    const utilityControl = screen.getByRole('button', { name: 'Expand Utility' })
    expect(timelineControl.getAttribute('aria-expanded')).toBe('false')
    expect(utilityControl.getAttribute('aria-expanded')).toBe('false')

    await user.click(timelineControl)

    expect(screen.getByRole('button', { name: 'Collapse Timeline' })).toBeTruthy()
    expect(screen.getByRole('link', { name: 'Photos' })).toBeTruthy()
    expect(screen.getByRole('button', { name: 'Expand Utility' })).toBeTruthy()

    rerender(
      <MemoryRouter initialEntries={['/albums']}>
        <Sidebar isCollapsed isMobileOpen toggleCollapse={vi.fn()} onNavigate={vi.fn()} />
      </MemoryRouter>
    )

    expect(screen.getByRole('button', { name: 'Collapse Timeline' })).toBeTruthy()
    expect(screen.getByRole('button', { name: 'Expand Utility' })).toBeTruthy()
  })
})
