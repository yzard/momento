import { cleanup, render, screen } from '@testing-library/react'
import { afterEach, describe, expect, it } from 'vitest'

import { PageFrame, PageHeader } from '../../../../src/frontend/components/layout/PageLayout'

describe('PageLayout', () => {
  afterEach(cleanup)

  it('uses the available page width with responsive browser-edge padding', () => {
    render(
      <PageFrame className="pb-28">
        <p>Page content</p>
      </PageFrame>
    )

    const frame = screen.getByText('Page content').parentElement
    expect(frame?.className).toContain('w-full')
    expect(frame?.className).toContain('px-4')
    expect(frame?.className).toContain('md:px-8')
    expect(frame?.className).toContain('xl:px-10')
    expect(frame?.className).not.toContain('max-w-')
    expect(frame?.className).not.toContain('mx-auto')
    expect(frame?.className).toContain('pb-28')
  })

  it('keeps the title on the left and actions in the trailing header region', () => {
    render(
      <PageHeader
        title="Timeline"
        description="Browse your library."
        actions={<button type="button">Select</button>}
      />
    )

    const header = screen.getByRole('banner')
    expect(header.className).toContain('lg:justify-between')
    expect(screen.getByRole('heading', { name: 'Timeline' })).toBeTruthy()
    expect(screen.getByText('Browse your library.')).toBeTruthy()
    expect(screen.getByRole('button', { name: 'Select' })).toBeTruthy()
  })
})
