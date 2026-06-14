import { useState, useEffect, useRef, useCallback } from 'react'

const WS_URL = 'ws://127.0.0.1:9001'
const HISTORY_SIZE = 60 // Keep 60 seconds of history

const emptySnapshot = {
  ops_per_sec: 0,
  p50_us: 0,
  p99_us: 0,
  p999_us: 0,
  used_slots: 0,
  total_slots: 65536,
  fill_pct: 0,
  breakdown: {},
}

export function useMetrics() {
  const [connected, setConnected] = useState(false)
  const [latest, setLatest] = useState(emptySnapshot)
  const [history, setHistory] = useState([]) // array of snapshots, max HISTORY_SIZE
  const [error, setError] = useState(null)
  const wsRef = useRef(null)
  const reconnectTimer = useRef(null)

  const connect = useCallback(() => {
    if (wsRef.current?.readyState === WebSocket.OPEN) return

    try {
      const ws = new WebSocket(WS_URL)
      wsRef.current = ws

      ws.onopen = () => {
        setConnected(true)
        setError(null)
        if (reconnectTimer.current) {
          clearTimeout(reconnectTimer.current)
          reconnectTimer.current = null
        }
      }

      ws.onmessage = (event) => {
        try {
          const data = JSON.parse(event.data)

          // Ignore the welcome message
          if (data.type === 'connected') return

          // Stamp with client-side time for x-axis
          const snapshot = { ...data, ts: Date.now() }

          setLatest(snapshot)
          setHistory(prev => {
            const next = [...prev, snapshot]
            return next.length > HISTORY_SIZE ? next.slice(-HISTORY_SIZE) : next
          })
        } catch {
          // Ignore malformed messages
        }
      }

      ws.onerror = () => {
        setError('WebSocket error — is MemLane running?')
      }

      ws.onclose = () => {
        setConnected(false)
        // Auto-reconnect every 2 seconds
        reconnectTimer.current = setTimeout(connect, 2000)
      }
    } catch (e) {
      setError(`Failed to connect: ${e.message}`)
      reconnectTimer.current = setTimeout(connect, 2000)
    }
  }, [])

  useEffect(() => {
    connect()
    return () => {
      if (wsRef.current) wsRef.current.close()
      if (reconnectTimer.current) clearTimeout(reconnectTimer.current)
    }
  }, [connect])

  return { connected, latest, history, error }
}