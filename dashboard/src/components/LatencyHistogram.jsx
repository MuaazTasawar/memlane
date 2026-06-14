import React from 'react'
import {
  LineChart,
  Line,
  XAxis,
  YAxis,
  CartesianGrid,
  Tooltip,
  Legend,
  ResponsiveContainer,
} from 'recharts'

function formatTime(ts) {
  const d = new Date(ts)
  return `${d.getMinutes().toString().padStart(2, '0')}:${d.getSeconds().toString().padStart(2, '0')}`
}

const CustomTooltip = ({ active, payload, label }) => {
  if (!active || !payload?.length) return null
  return (
    <div style={tooltipStyle}>
      <div style={{ color: '#94a3b8', marginBottom: 6, fontSize: 12 }}>{label}</div>
      {payload.map(p => (
        <div key={p.dataKey} style={{ color: p.color, fontSize: 13 }}>
          {p.name}: <strong>{p.value} µs</strong>
        </div>
      ))}
    </div>
  )
}

const tooltipStyle = {
  background: '#0f172a',
  border: '1px solid #334155',
  borderRadius: 8,
  padding: '10px 14px',
}

export default function LatencyHistogram({ history }) {
  const data = history.map(h => ({
    ts: formatTime(h.ts),
    p50: h.p50_us,
    p99: h.p99_us,
    p999: h.p999_us,
  }))

  const maxLatency = Math.max(
    ...history.map(h => h.p999_us),
    1
  )

  return (
    <div style={styles.card}>
      <div style={styles.label}>LATENCY OVER TIME (µs)</div>

      {/* Current latency badges */}
      <div style={styles.badges}>
        {[
          { label: 'P50', value: history.at(-1)?.p50_us ?? 0, color: '#4ade80' },
          { label: 'P99', value: history.at(-1)?.p99_us ?? 0, color: '#f59e0b' },
          { label: 'P99.9', value: history.at(-1)?.p999_us ?? 0, color: '#f87171' },
        ].map(({ label, value, color }) => (
          <div key={label} style={styles.badge}>
            <span style={{ ...styles.badgeLabel, color }}>{label}</span>
            <span style={styles.badgeValue}>{value} <span style={styles.unit}>µs</span></span>
          </div>
        ))}
        <div style={styles.badge}>
          <span style={{ ...styles.badgeLabel, color: '#94a3b8' }}>Note</span>
          <span style={{ ...styles.badgeValue, fontSize: 12, color: '#64748b' }}>
            Redis P50 ≈ 50–300 µs
          </span>
        </div>
      </div>

      {data.length < 2 ? (
        <div style={styles.waiting}>Waiting for data — run some operations...</div>
      ) : (
        <ResponsiveContainer width="100%" height={220}>
          <LineChart data={data} margin={{ top: 8, right: 16, left: 0, bottom: 0 }}>
            <CartesianGrid strokeDasharray="3 3" stroke="#1e293b" />
            <XAxis
              dataKey="ts"
              tick={{ fill: '#64748b', fontSize: 11 }}
              tickLine={false}
              axisLine={{ stroke: '#334155' }}
            />
            <YAxis
              tick={{ fill: '#64748b', fontSize: 11 }}
              tickLine={false}
              axisLine={false}
              domain={[0, maxLatency + 1]}
              tickFormatter={v => `${v}µs`}
              width={52}
            />
            <Tooltip content={<CustomTooltip />} />
            <Legend
              wrapperStyle={{ fontSize: 12, color: '#94a3b8' }}
            />
            <Line
              type="monotone"
              dataKey="p50"
              name="P50"
              stroke="#4ade80"
              strokeWidth={2}
              dot={false}
              isAnimationActive={false}
            />
            <Line
              type="monotone"
              dataKey="p99"
              name="P99"
              stroke="#f59e0b"
              strokeWidth={2}
              dot={false}
              isAnimationActive={false}
            />
            <Line
              type="monotone"
              dataKey="p999"
              name="P99.9"
              stroke="#f87171"
              strokeWidth={2}
              dot={false}
              isAnimationActive={false}
            />
          </LineChart>
        </ResponsiveContainer>
      )}
    </div>
  )
}

const styles = {
  card: {
    background: '#0f172a',
    border: '1px solid #334155',
    borderRadius: 12,
    padding: 24,
  },
  label: {
    fontSize: 11,
    letterSpacing: '0.15em',
    color: '#64748b',
    marginBottom: 16,
  },
  badges: {
    display: 'flex',
    gap: 24,
    marginBottom: 20,
    flexWrap: 'wrap',
  },
  badge: {
    display: 'flex',
    flexDirection: 'column',
    gap: 2,
  },
  badgeLabel: {
    fontSize: 11,
    letterSpacing: '0.1em',
    fontWeight: 700,
  },
  badgeValue: {
    fontSize: 20,
    color: '#e2e8f0',
    fontWeight: 600,
  },
  unit: {
    fontSize: 12,
    color: '#64748b',
  },
  waiting: {
    height: 220,
    display: 'flex',
    alignItems: 'center',
    justifyContent: 'center',
    color: '#475569',
    fontStyle: 'italic',
    fontSize: 14,
  },
}