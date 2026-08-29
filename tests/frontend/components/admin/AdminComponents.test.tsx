import { cleanup, render, screen } from '@testing-library/react'
import { Database } from '../../../../src/frontend/node_modules/lucide-react'
import { afterEach, describe, expect, it } from 'vitest'

import {
  AdminFailureLog,
  AdminPanel,
  AdminStatusMetrics,
} from '../../../../src/frontend/components/admin/AdminComponents'

describe('AdminComponents', () => {
  afterEach(cleanup)

  it('renders a consistent panel and status metrics with unloaded placeholders', () => {
    render(
      <AdminPanel icon={Database} title="Import" description="Import description">
        <AdminStatusMetrics
          metrics={[
            { label: 'Status', value: null },
            { label: 'Failed', value: 2, emphasis: true },
          ]}
        />
      </AdminPanel>
    )

    expect(screen.getByRole('heading', { name: 'Import' })).toBeTruthy()
    expect(screen.getByText('Status').parentElement?.textContent).toContain('—')
    expect(screen.getByText('Failed').parentElement?.textContent).toContain('2')
  })

  it('uses a read-only selectable text box for complete failure output', () => {
    render(<AdminFailureLog title="Failure log" entries={['first', 'second']} />)

    const log = screen.getByLabelText('Failure log') as HTMLTextAreaElement
    expect(log.readOnly).toBe(true)
    expect(log.value).toBe('first\nsecond')
  })
})
