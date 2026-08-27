import { cleanup, render, screen } from '@testing-library/react'
import { afterEach, describe, expect, it } from 'vitest'
import AndroidAppDownloadLink from '../../../../src/frontend/components/layout/AndroidAppDownloadLink'

describe('AndroidAppDownloadLink', () => {
  afterEach(cleanup)

  it('downloads the APK from the stable same-origin path', () => {
    render(<AndroidAppDownloadLink compact={false} />)

    const downloadLink = screen.getByRole('link', { name: 'Download Android app' })
    expect(downloadLink.getAttribute('href')).toBe('/momento-android.apk')
    expect(downloadLink.getAttribute('download')).toBe('momento-android.apk')
    expect(downloadLink.textContent).toContain('Download Android app')
  })

  it('keeps an accessible download action in compact navigation', () => {
    render(<AndroidAppDownloadLink compact />)

    const downloadLink = screen.getByRole('link', { name: 'Download Android app' })
    expect(downloadLink.textContent).toBe('')
    expect(downloadLink.getAttribute('title')).toBe('Download Android app')
  })
})
