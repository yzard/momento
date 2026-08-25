import { defineConfig } from 'vitest/config'
import react from '@vitejs/plugin-react'
import { readFileSync } from 'node:fs'
import { fileURLToPath, URL } from 'node:url'

const version = readFileSync(fileURLToPath(new URL('../backend/version.txt', import.meta.url)), 'utf8').trim()

export default defineConfig({
  define: {
    __MOMENTO_VERSION__: JSON.stringify(version),
  },
  plugins: [react()],
  resolve: {
    alias: {
      '@tanstack/react-query': fileURLToPath(new URL('./node_modules/@tanstack/react-query', import.meta.url)),
      '@testing-library/react': fileURLToPath(new URL('./node_modules/@testing-library/react', import.meta.url)),
      '@testing-library/user-event': fileURLToPath(new URL('./node_modules/@testing-library/user-event', import.meta.url)),
      'axios': fileURLToPath(new URL('./node_modules/axios/index.js', import.meta.url)),
      'react-router-dom': fileURLToPath(new URL('./node_modules/react-router-dom', import.meta.url)),
    },
  },
  test: {
    environment: 'jsdom',
    include: ['../../tests/frontend/**/*.test.{ts,tsx}'],
  },
  server: {
    proxy: {
      '/api': {
        target: 'http://localhost:8000',
        changeOrigin: true,
      },
      '/webdav': {
        target: 'http://localhost:8000',
        changeOrigin: true,
      },
    },
  },
})
