import React from 'react'
import { useMetrics } from './hooks/useMetrics.js'
import OpsCounter from './components/OpsCounter.jsx'
import LatencyHistogram from './components/LatencyHistogram.jsx'
import CompareBar from './components/CompareBar.jsx'

export default function App() {
  const { connected, latest, history, error } = useMetrics()

  return (
    <div style={styles.root}>
      {/* Header */}
      <header style={styles.header}>
        <div style={styles.headerLeft}>
          <div style={styles.logo}>⚡ MemLane</div>
          <div style={styles.tagline}>Zero-TCP Shared Memory Cache</div>
        </div>
        <div style={styles.headerRight}>
          <div style={connected ? styles.statusOn : styles.statusOff}>
            <div style={connected ? styles.dotOn : styles.dotOff} />
            {connected ? 'Live' : 'Disconnected'}
          </div>
          {error && <div style={styles.error}>{error}</div>}
        </div>
      </header>

      {/* Connection banner */}
      {!connected && (
        <div style={styles.banner}>
          <strong>MemLane not running.</strong> Start it with:{' '}
          <code style={styles.code}>cargo run --example basic_usage</code>
          {' '}then refresh this page.
        </div>
      )}

      {/* Main grid */}
      <main style={styles.grid}>
        {/* Row 1: Big ops counter (spans 2 cols) */}
        <OpsCounter latest={latest} history={history} />

        {/* Row 2: Latency chart + Compare bar */}
        <LatencyHistogram history={history} />
        <div style={styles.slotCard}>
          <div style={styles.slotLabel}>ARENA CAPACITY</div>
          <div style={styles.slotBig}>
            {latest.used_slots.toLocaleString()}
            <span style={styles.slotOf}> / {latest.total_slots.toLocaleString()}</span>
          </div>
          <div style={styles.fillBarOuter}>
            <div
              style={{
                ...styles.fillBarInner,
                width: `${Math.min(latest.fill_pct, 100)}%`,
                background: latest.fill_pct > 80
                  ? '#f87171'
                  : latest.fill_pct > 50
                    ? '#f59e0b'
                    : '#4ade80',
              }}
            />
          </div>
          <div style={styles.fillPct}>{latest.fill_pct.toFixed(2)}% full</div>

          {/* Latency badges */}
          <div style={styles.latencyGrid}>
            {[
              { label: 'P50 latency', value: latest.p50_us, color: '#4ade80' },
              { label: 'P99 latency', value: latest.p99_us, color: '#f59e0b' },
              { label: 'P99.9 latency', value: latest.p999_us, color: '#f87171' },
            ].map(({ label, value, color }) => (
              <div key={label} style={styles.latencyItem}>
                <div style={{ ...styles.latencyLabel, color }}>{label}</div>
                <div style={styles.latencyValue}>{value} µs</div>
              </div>
            ))}
          </div>
        </div>

        {/* Row 3: Comparison bar (spans 2 cols) */}
        <CompareBar latest={latest} />
      </main>

      {/* Footer */}
      <footer style={styles.footer}>
        <span>MemLane v0.1.0</span>
        <span style={styles.sep}>·</span>
        <span>TCP: <code style={styles.code}>127.0.0.1:6399</code></span>
        <span style={styles.sep}>·</span>
        <span>WS: <code style={styles.code}>ws://127.0.0.1:9001</code></span>
        <span style={styles.sep}>·</span>
        <a
          href="https://github.com/MuaazTasawar/memlane"
          style={styles.link}
          target="_blank"
          rel="noreferrer"
        >
          GitHub ↗
        </a>
      </footer>
    </div>
  )
}

const styles = {
  root: {
    minHeight: '100vh',
    display: 'flex',
    flexDirection: 'column',
    background: '#0a0e1a',
    color: '#e2e8f0',
    fontFamily: "'JetBrains Mono', 'Fira Code', 'Courier New', monospace",
  },
  header: {
    display: 'flex',
    alignItems: 'center',
    justifyContent: 'space-between',
    padding: '20px 32px',
    borderBottom: '1px solid #1e293b',
    background: '#0d1120',
  },
  headerLeft: {
    display: 'flex',
    flexDirection: 'column',
    gap: 4,
  },
  logo: {
    fontSize: 22,
    fontWeight: 700,
    color: '#38bdf8',
    letterSpacing: '-0.5px',
  },
  tagline: {
    fontSize: 12,
    color: '#64748b',
    letterSpacing: '0.05em',
  },
  headerRight: {
    display: 'flex',
    flexDirection: 'column',
    alignItems: 'flex-end',
    gap: 6,
  },
  statusOn: {
    display: 'flex',
    alignItems: 'center',
    gap: 8,
    fontSize: 13,
    color: '#4ade80',
    fontWeight: 600,
  },
  statusOff: {
    display: 'flex',
    alignItems: 'center',
    gap: 8,
    fontSize: 13,
    color: '#f87171',
    fontWeight: 600,
  },
  dotOn: {
    width: 8,
    height: 8,
    borderRadius: '50%',
    background: '#4ade80',
    boxShadow: '0 0 6px #4ade80',
    animation: 'pulse 2s infinite',
  },
  dotOff: {
    width: 8,
    height: 8,
    borderRadius: '50%',
    background: '#f87171',
  },
  error: {
    fontSize: 11,
    color: '#f87171',
    maxWidth: 280,
    textAlign: 'right',
  },
  banner: {
    background: '#1c1008',
    borderBottom: '1px solid #78350f',
    padding: '12px 32px',
    fontSize: 13,
    color: '#fbbf24',
  },
  code: {
    background: '#1e293b',
    padding: '2px 6px',
    borderRadius: 4,
    fontSize: 12,
    color: '#7dd3fc',
  },
  grid: {
    flex: 1,
    display: 'grid',
    gridTemplateColumns: '1fr 1fr',
    gap: 16,
    padding: 24,
    alignContent: 'start',
  },
  slotCard: {
    background: '#0f172a',
    border: '1px solid #334155',
    borderRadius: 12,
    padding: 24,
  },
  slotLabel: {
    fontSize: 11,
    letterSpacing: '0.15em',
    color: '#64748b',
    marginBottom: 12,
  },
  slotBig: {
    fontSize: 36,
    fontWeight: 700,
    color: '#e2e8f0',
    marginBottom: 16,
  },
  slotOf: {
    fontSize: 18,
    color: '#64748b',
    fontWeight: 400,
  },
  fillBarOuter: {
    width: '100%',
    height: 8,
    background: '#1e293b',
    borderRadius: 4,
    overflow: 'hidden',
    marginBottom: 8,
  },
  fillBarInner: {
    height: '100%',
    borderRadius: 4,
    transition: 'width 0.5s ease',
  },
  fillPct: {
    fontSize: 12,
    color: '#64748b',
    marginBottom: 24,
  },
  latencyGrid: {
    display: 'flex',
    flexDirection: 'column',
    gap: 12,
    borderTop: '1px solid #1e293b',
    paddingTop: 16,
  },
  latencyItem: {
    display: 'flex',
    justifyContent: 'space-between',
    alignItems: 'center',
  },
  latencyLabel: {
    fontSize: 12,
    letterSpacing: '0.05em',
  },
  latencyValue: {
    fontSize: 16,
    fontWeight: 600,
    color: '#e2e8f0',
  },
  footer: {
    display: 'flex',
    gap: 0,
    alignItems: 'center',
    padding: '14px 32px',
    borderTop: '1px solid #1e293b',
    fontSize: 12,
    color: '#475569',
    background: '#0d1120',
    flexWrap: 'wrap',
    rowGap: 4,
  },
  sep: {
    margin: '0 12px',
    color: '#1e293b',
  },
  link: {
    color: '#38bdf8',
    textDecoration: 'none',
  },
}