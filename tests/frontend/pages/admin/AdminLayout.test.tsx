import { cleanup, render, screen } from '@testing-library/react'
import { MemoryRouter, Route, Routes } from '../../../../src/frontend/node_modules/react-router-dom'
import { afterEach, describe, expect, it, vi } from 'vitest'

vi.mock('../../../../src/frontend/components/admin/ImportPanel', () => ({
  default: () => <div>Local import controls</div>,
}))
vi.mock('../../../../src/frontend/components/admin/MetadataPanel', () => ({
  default: () => <div>Metadata controls</div>,
}))
vi.mock('../../../../src/frontend/components/admin/AiPanel', () => ({
  default: () => <div>AI controls</div>,
}))
vi.mock('../../../../src/frontend/components/admin/UserManagement', () => ({
  default: () => <div>User controls</div>,
}))

import AdminLayout, {
  AdminImportPage,
  AdminMetadataPage,
  AdminAIPage,
  AdminUsersPage,
} from '../../../../src/frontend/pages/admin/AdminLayout'

function renderAdmin(path: string) {
  render(
    <MemoryRouter initialEntries={[path]}>
      <Routes>
        <Route path="admin" element={<AdminLayout />}>
          <Route path="import" element={<AdminImportPage />} />
          <Route path="metadata" element={<AdminMetadataPage />} />
          <Route path="ai" element={<AdminAIPage />} />
          <Route path="users" element={<AdminUsersPage />} />
        </Route>
      </Routes>
    </MemoryRouter>
  )
}

describe('AdminLayout', () => {
  afterEach(cleanup)

  it('stacks WebDAV above Local Import as peer panels', () => {
    renderAdmin('/admin/import')

    const localImport = screen.getByRole('heading', { name: 'Local Import' }).closest('section')
    const webDAV = screen.getByRole('heading', { name: 'WebDAV' }).closest('section')
    expect(localImport).not.toBe(webDAV)
    expect(webDAV?.parentElement?.classList.contains('space-y-6')).toBe(true)
    expect(webDAV?.parentElement).toBe(localImport?.parentElement)
    expect(
      webDAV?.compareDocumentPosition(localImport as Node) & Node.DOCUMENT_POSITION_FOLLOWING
    ).toBeTruthy()
    expect(screen.getByText('Local import controls')).toBeTruthy()
    expect(screen.getByText(new URL('/webdav/', window.location.origin).toString())).toBeTruthy()
  })

  it.each([
    ['/admin/metadata', 'Metadata controls'],
    ['/admin/ai', 'AI controls'],
    ['/admin/users', 'User controls'],
  ])('renders the requested child page at %s', (path, content) => {
    renderAdmin(path)
    expect(screen.getByText(content)).toBeTruthy()
  })
})
