import { readFileSync } from 'node:fs'
import { URL as FileURL } from 'node:url'
const css = readFileSync(
  new FileURL('../../../src/frontend/styles/index.css', import.meta.url),
  'utf8'
)
import { describe, expect, it } from 'vitest'

const neutral = [
  'background',
  'foreground',
  'card',
  'card-foreground',
  'popover',
  'popover-foreground',
  'primary',
  'primary-foreground',
  'secondary',
  'secondary-foreground',
  'muted',
  'muted-foreground',
  'accent',
  'accent-foreground',
  'border',
  'input',
  'ring',
]

function luminance(lightness: number) {
  const channel = lightness / 100
  return channel <= 0.04045 ? channel / 12.92 : ((channel + 0.055) / 1.055) ** 2.4
}

describe.each([':root', '.dark'])('%s interface palette', (selector) => {
  const block = css.slice(css.indexOf(`${selector} {`)).split('}')[0] ?? ''
  const tokens = Object.fromEntries(
    [...block.matchAll(/--([\w-]+):\s*([\d.]+)\s+([\d.]+)%\s+([\d.]+)%/g)].map((m) => [
      m[1],
      [Number(m[2]), Number(m[3]), Number(m[4])],
    ])
  )

  it('keeps interface colors achromatic and the canvas black or white', () => {
    for (const name of neutral) {
      expect(tokens[name], name).toBeDefined()
      expect(tokens[name]?.[1], name).toBe(0)
    }
    expect(tokens.background?.[2]).toBe(selector === '.dark' ? 0 : 100)
  })

  it.each([
    ['foreground', 'background'],
    ['card-foreground', 'card'],
    ['popover-foreground', 'popover'],
    ['primary-foreground', 'primary'],
    ['secondary-foreground', 'secondary'],
    ['muted-foreground', 'background'],
  ])('keeps %s readable against %s', (foreground, background) => {
    const first = luminance(tokens[foreground]?.[2] ?? NaN)
    const second = luminance(tokens[background]?.[2] ?? NaN)
    expect(
      (Math.max(first, second) + 0.05) / (Math.min(first, second) + 0.05)
    ).toBeGreaterThanOrEqual(4.5)
  })
})
