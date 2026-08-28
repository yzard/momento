import { describe, expect, it } from 'vitest'

import { createClusterIconElement } from '../../../../src/frontend/components/map/clusterIcon'

describe('createClusterIconElement', () => {
  it('constructs marker content without interpreting thumbnail values as HTML', () => {
    const marker = createClusterIconElement('photo.jpg" onerror="alert(1)', 7)
    const image = marker.querySelector('img')

    expect(image).toBeTruthy()
    expect(image?.getAttribute('onerror')).toBeNull()
    expect(marker.querySelector('.map-marker__badge')?.textContent).toBe('7')
  })

  it('uses a placeholder and omits the badge for a single item', () => {
    const marker = createClusterIconElement(null, 1)

    expect(marker.querySelector('.map-marker__placeholder')).toBeTruthy()
    expect(marker.querySelector('.map-marker__badge')).toBeNull()
  })
})
