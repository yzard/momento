import { cleanup, render, screen } from '@testing-library/react'
import { afterEach, describe, expect, it, vi } from 'vitest'

vi.mock('../../../src/frontend/components/admin/ImportPanel', () => ({
  default: () => <div data-testid="import-panel" />,
}))
vi.mock('../../../src/frontend/components/admin/MetadataPanel', () => ({
  default: () => <div data-testid="metadata-panel" />,
}))
vi.mock('../../../src/frontend/components/admin/AiPanel', () => ({
  default: () => <div data-testid="ai-panel" />,
}))
vi.mock('../../../src/frontend/components/admin/UserManagement', () => ({
  default: () => <div data-testid="user-panel" />,
}))

import Admin from '../../../src/frontend/pages/Admin'

afterEach(cleanup)

describe('Admin', () => {
  it('places compact local import beside the wider metadata section', () => {
    render(<Admin />)

    const importHeading = screen.getByRole('heading', { name: 'Local Import' })
    const metadataHeading = screen.getByRole('heading', { name: 'Metadata' })
    const operationsGrid = importHeading.closest('section')?.parentElement
    const webdavUrl = new URL('/webdav/', window.location.origin).toString()

    expect(operationsGrid?.className).toContain('md:grid-cols-[minmax(15rem,0.72fr)_minmax(0,1.5fr)]')
    expect(screen.getByText('/data/imports/')).toBeTruthy()
    expect(screen.getByText(webdavUrl)).toBeTruthy()
    expect(screen.getByText(/with your Momento username and password/)).toBeTruthy()
    expect(screen.getByText('Generate metadata.')).toBeTruthy()
    expect(screen.getAllByRole('heading', { name: 'Local Import' })).toHaveLength(1)
    expect(screen.getAllByRole('heading', { name: 'Metadata' })).toHaveLength(1)
    expect(metadataHeading.closest('section')).not.toBe(importHeading.closest('section'))
  })
})
