import React from 'react'

const REDIS_BASELINE = 400_000 // ~400K ops/sec typical Redis localhost

function formatOps(n) {
  if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(2)}M`
  if (n >= 1_000) return `${(n / 1_000).toFixed(1)}K`
  return `${n}`
}

export default function OpsCounter({ latest, history }) {
  const ops = latest.ops_per_sec
  const speedup = ops > 0 ? (ops / REDIS_BASELINE).toFixed(1) : '—'
  const peak = history.length > 0
    ? Math.max(...history.map(h => h.ops_per_sec))
    : 0

  return (
    <div style={styles.card}>
      <div style={styles.label}>THROUGHPUT</div>

      {/* Big ops/sec number */}
      <div style={styles.bigNumber}>
        {formatOps(ops)}
        <span style={styles.unit}> ops/sec</span>
      </div>

      {/* Speedup vs Redis */}
      <div style={styles.speedup}>
        {speedup !== '—'
          ? <><span style={styles.speedupNum}>{speedup}×</span> faster than Redis TCP</>
          : <span style={styles.waiting}>Waiting for data...</span>
        }
      </div>

      {/* Stats row */}
      <div style={styles.statsRow}>
        <div style={styles.stat}>
          <div style={styles.statLabel}>Peak (60s)</div>
          <div style={styles.statValue}>{formatOps(peak)}</div>
        </div>
        <div style={styles.stat}>
          <div style={styles.statLabel}>Redis baseline</div>
          <div style={styles.statValue}>{formatOps(REDIS_BASELINE)}</div>
        </div>
        <div style={styles.stat}>
          <div style={styles.statLabel}>Arena fill</div>
          <div style={styles.statValue}>{latest.fill_pct.toFixed(1)}%</div>
        </div>
        <div style={styles.stat}>
          <div style={styles.statLabel}>Used slots</div>
          <div style={styles.statValue}>{latest.used_slots.toLocaleString()}</div>
        </div>
      </div>

      {/* Command breakdown */}
      {Object.keys(latest.breakdown).length > 0 && (
        <div style={styles.breakdown}>
          <div style={styles.breakdownLabel}>Command breakdown</div>
          <div style={styles.breakdownRow}>
            {Object.entries(latest.breakdown)
              .sort((a, b) => b[1] - a[1])
              .map(([cmd, count]) => (
                <div key={cmd} style={styles.breakdownItem}>
                  <span style={styles.cmd}>{cmd}</span>
                  <span style={styles.cmdCount}>{formatOps(count)}</span>
                </div>
              ))}
          </div>
        </div>
      )}
    </div>
  )
}

const styles = {
  card: {
    background: 'linear-gradient(135deg, #0f172a 0%, #1e293b 100%)',
    border: '1px solid #334155',
    borderRadius: 12,
    padding: 28,
    gridColumn: 'span 2',
  },
  label: {
    fontSize: 11,
    letterSpacing: '0.15em',
    color: '#64748b',
    marginBottom: 12,
  },
  bigNumber: {
    fontSize: 64,
    fontWeight: 700,
    color: '#38bdf8',
    lineHeight: 1,
    marginBottom: 8,
  },
  unit: {
    fontSize: 24,
    color: '#94a3b8',
    fontWeight: 400,
  },
  speedup: {
    fontSize: 18,
    color: '#94a3b8',
    marginBottom: 24,
  },
  speedupNum: {
    color: '#4ade80',
    fontWeight: 700,
    fontSize: 22,
  },
  waiting: {
    color: '#475569',
    fontStyle: 'italic',
  },
  statsRow: {
    display: 'flex',
    gap: 32,
    marginBottom: 20,
    flexWrap: 'wrap',
  },
  stat: {
    display: 'flex',
    flexDirection: 'column',
    gap: 4,
  },
  statLabel: {
    fontSize: 11,
    color: '#64748b',
    letterSpacing: '0.1em',
  },
  statValue: {
    fontSize: 18,
    color: '#e2e8f0',
    fontWeight: 600,
  },
  breakdown: {
    borderTop: '1px solid #1e293b',
    paddingTop: 16,
  },
  breakdownLabel: {
    fontSize: 11,
    color: '#64748b',
    letterSpacing: '0.1em',
    marginBottom: 10,
  },
  breakdownRow: {
    display: 'flex',
    gap: 16,
    flexWrap: 'wrap',
  },
  breakdownItem: {
    display: 'flex',
    flexDirection: 'column',
    gap: 2,
  },
  cmd: {
    fontSize: 11,
    color: '#f59e0b',
    letterSpacing: '0.1em',
  },
  cmdCount: {
    fontSize: 16,
    color: '#e2e8f0',
    fontWeight: 600,
  },
}