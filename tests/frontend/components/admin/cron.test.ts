import { describe, expect, it } from 'vitest'

import {
  joinCronFields,
  splitCronExpression,
  validCronFields,
} from '../../../../src/frontend/components/admin/cron'

describe('cron fields', () => {
  it('splits and rejoins exactly five fields', () => {
    const fields = ['15', '1', '*', '*', '1-5'] as const

    expect(splitCronExpression(' 15  1 * * 1-5 ')).toEqual(fields)
    expect(joinCronFields([...fields])).toBe('15 1 * * 1-5')
    expect(validCronFields([...fields])).toBe(true)
  })

  it('rejects missing fields and embedded whitespace', () => {
    expect(validCronFields(['15 30', '1', '*', '*', '1-5'])).toBe(false)
    expect(splitCronExpression('15 1 * *')).toEqual(['', '', '', '', ''])
  })
})
