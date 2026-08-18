import { defineConfig } from 'vitest/config'
import react from '@vitejs/plugin-react'
import { fileURLToPath, URL } from 'node:url'

export default defineConfig({
  plugins: [react()],
  resolve: {
    alias: {
      '@tanstack/react-query': fileURLToPath(new URL('./node_modules/@tanstack/react-query', import.meta.url)),
      '@testing-library/react': fileURLToPath(new URL('./node_modules/@testing-library/react', import.meta.url)),
      '@testing-library/user-event': fileURLToPath(new URL('./node_modules/@testing-library/user-event', import.meta.url)),
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
