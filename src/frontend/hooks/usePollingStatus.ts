import { useCallback, useEffect, useRef, useState } from 'react'

export function usePollingStatus<T>(
  load: () => Promise<T>,
  loadErrorMessage: string,
  intervalMilliseconds: number,
) {
  const requestGeneration = useRef(0)
  const [status, setStatus] = useState<T | null>(null)
  const [errorMessage, setErrorMessage] = useState<string | null>(null)

  const refresh = useCallback(async () => {
    const generation = requestGeneration.current + 1
    requestGeneration.current = generation
    try {
      const nextStatus = await load()
      if (generation !== requestGeneration.current) return
      setStatus(nextStatus)
      setErrorMessage(null)
    } catch {
      if (generation !== requestGeneration.current) return
      setErrorMessage(loadErrorMessage)
    }
  }, [load, loadErrorMessage])

  useEffect(() => {
    void refresh()
    const timer = window.setInterval(() => void refresh(), intervalMilliseconds)
    return () => {
      window.clearInterval(timer)
      requestGeneration.current += 1
    }
  }, [intervalMilliseconds, refresh])

  return { status, errorMessage, setErrorMessage, refresh }
}
