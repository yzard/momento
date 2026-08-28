import { render, screen } from '@testing-library/react'
import { describe, expect, it } from 'vitest'

import PageState from '../../../../src/frontend/components/common/PageState'

describe('PageState', () => {
  it('renders a reusable title, description, icon, and action', () => {
    render(
      <PageState
        icon={<span>Icon</span>}
        title="Nothing here"
        description="Try another view."
        action={<button type="button">Try again</button>}
      />
    )

    expect(screen.getByRole('heading', { name: 'Nothing here' })).toBeTruthy()
    expect(screen.getByText('Try another view.')).toBeTruthy()
    expect(screen.getByText('Icon')).toBeTruthy()
    expect(screen.getByRole('button', { name: 'Try again' })).toBeTruthy()
  })
})
