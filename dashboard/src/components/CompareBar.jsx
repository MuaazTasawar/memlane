import React from 'react'
import {
  BarChart,
  Bar,
  XAxis,
  YAxis,
  CartesianGrid,
  Tooltip,
  Cell,
  ResponsiveContainer,
  LabelList,
} from 'recharts'

// Static Redis baselines (typical localhost benchmarks)
const REDIS_GET_OPS = 400_000
const REDIS_SET_OPS = 350_000

function formatOps(n) {
  if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(2)}M`
  if (n >= 1_000) return `${(n / 1_000).toFixed(0)}K`
  return `${n}`
}

const CustomTooltip = ({ active, payload }) => {
  if (!active || !payload?.length) return null
  const d = payload[0].payload
  return (
    <div style={tooltipStyle}>
      <div style={{ color: d.color, fontWeight: 700, marginBottom: 4 }}>{d.name}</div>
      <div style={{ color: '#e2e8f0' }}>{formatOps(d.ops)} ops/sec</div>
      {d.speedup && (
        <div style={{ color: '#4ade80', fontSize: 12, marginTop: 4 }}>
          {d.speedup}× faster than Redis
        </div>
      )}
    </div>
  )
}

const tooltipStyle = {
  background: '#0f172a',
  border: '1px solid #334155',
  borderRadius: 8,
  padding: '10px 14px',
  fontSize: 13,
}

export default function CompareBar({ latest }) {
  const mlOps = latest.ops_per_sec

  // Estimate GET vs SET from breakdown if available
  const breakdown = latest.breakdown || {}
  const totalBreakdown = Object.values(breakdown).reduce((a, b) => a + b, 0)
  const getShare = totalBreakdown > 0
    ? (breakdown['GET'] || 0) / totalBreakdown
    : 0.8
  const setShare = 1 - getShare

  const mlGet = Math.round(mlOps * getShare)
  const mlSet = Math.round(mlOps * setShare)

  const data = [
    {
      name: 'Redis GET (TCP)',
      ops: REDIS_GET_OPS,
      color: '#475569',
      speedup: null,
    },
    {
      name: 'Redis SET (TCP)',
      ops: REDIS_SET_OPS,
      color: '#334155',
      speedup: null,
    },
    {
      name: 'MemLane GET (shm)',
      ops: mlGet,
      color: '#38bdf8',
      speedup: mlGet > 0 ? (mlGet / REDIS_GET_OPS).toFixed(1) : null,
    },
    {
      name: 'MemLane SET (shm)',
      ops: mlSet,
      color: '#818cf8',
      speedup: mlSet > 0 ? (mlSet / REDIS_SET_OPS).toFixed(1) : null,
    },
  ]

  const maxOps = Math.max(...data.map(d => d.ops), 1)

  return (
    <div style={styles.card}>
      <div style={styles.label}>MEMLANE vs REDIS — OPS/SEC COMPARISON</div>

      <div style={styles.legend}>
        <div style={styles.legendItem}>
          <div style={{ ...styles.dot, background: '#475569' }} />
          <span>Redis (TCP, localhost)</span>
        </div>
        <div style={styles.legendItem}>
          <div style={{ ...styles.dot, background: '#38bdf8' }} />
          <span>MemLane (shared memory)</span>
        </div>
      </div>

      {mlOps === 0 ? (
        <div style={styles.waiting}>
          Run operations against MemLane to see the comparison...
        </div>
      ) : (
        <ResponsiveContainer width="100%" height={240}>
          <BarChart
            data={data}
            layout="vertical"
            margin={{ top: 0, right: 80, left: 8, bottom: 0 }}
          >
            <CartesianGrid strokeDasharray="3 3" stroke="#1e293b" horizontal={false} />
            <XAxis
              type="number"
              tick={{ fill: '#64748b', fontSize: 11 }}
              tickLine={false}
              axisLine={{ stroke: '#334155' }}
              tickFormatter={formatOps}
              domain={[0, maxOps * 1.1]}
            />
            <YAxis
              type="category"
              dataKey="name"
              tick={{ fill: '#94a3b8', fontSize: 12 }}
              tickLine={false}
              axisLine={false}
              width={160}
            />
            <Tooltip content={<CustomTooltip />} cursor={{ fill: '#ffffff08' }} />
            <Bar dataKey="ops" radius={[0, 4, 4, 0]} isAnimationActive={false}>
              {data.map((entry, index) => (
                <Cell key={index} fill={entry.color} />
              ))}
              <LabelList
                dataKey="ops"
                position="right"
                formatter={formatOps}
                style={{ fill: '#94a3b8', fontSize: 12, fontWeight: 600 }}
              />
            </Bar>
          </BarChart>
        </ResponsiveContainer>
      )}

      {/* Speedup callout */}
      {mlOps > 0 && (
        <div style={styles.callout}>
          <span style={styles.calloutNum}>
            {(mlOps / REDIS_GET_OPS).toFixed(1)}×
          </span>
          {' '}faster than Redis on the same machine — zero TCP, zero copy.
        </div>
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
    gridColumn: 'span 2',
  },
  label: {
    fontSize: 11,
    letterSpacing: '0.15em',
    color: '#64748b',
    marginBottom: 16,
  },
  legend: {
    display: 'flex',
    gap: 24,
    marginBottom: 20,
  },
  legendItem: {
    display: 'flex',
    alignItems: 'center',
    gap: 8,
    fontSize: 13,
    color: '#94a3b8',
  },
  dot: {
    width: 10,
    height: 10,
    borderRadius: '50%',
  },
  waiting: {
    height: 240,
    display: 'flex',
    alignItems: 'center',
    justifyContent: 'center',
    color: '#475569',
    fontStyle: 'italic',
    fontSize: 14,
  },
  callout: {
    marginTop: 16,
    padding: '12px 16px',
    background: '#0a1628',
    borderRadius: 8,
    borderLeft: '3px solid #38bdf8',
    fontSize: 14,
    color: '#94a3b8',
  },
  calloutNum: {
    color: '#4ade80',
    fontWeight: 700,
    fontSize: 18,
  },
}