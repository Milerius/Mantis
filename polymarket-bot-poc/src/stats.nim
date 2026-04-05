# polymarket-bot-poc/src/stats.nim — Rolling statistics for telemetry
#
# All structures are single-threaded — only used within telemetry thread.

import std/[algorithm]
import types

# ── Rolling rate counter (events per second) ──

const RateBuckets = 10  # 100ms buckets, 10 = 1 second window

type
  RollingCounter* = object
    buckets: array[RateBuckets, int32]
    currentBucket: int
    lastBucketMs: int64

proc init*(rc: var RollingCounter, nowMs: int64) =
  rc.lastBucketMs = nowMs
  rc.currentBucket = 0
  for i in 0..<RateBuckets: rc.buckets[i] = 0

proc tick*(rc: var RollingCounter, nowMs: int64) =
  let elapsed = nowMs - rc.lastBucketMs
  if elapsed >= 100:
    let advance = min(int(elapsed div 100), RateBuckets)
    for i in 0..<advance:
      rc.currentBucket = (rc.currentBucket + 1) mod RateBuckets
      rc.buckets[rc.currentBucket] = 0
    rc.lastBucketMs = nowMs

proc increment*(rc: var RollingCounter, nowMs: int64, count: int32 = 1) =
  rc.tick(nowMs)
  rc.buckets[rc.currentBucket] += count

proc rate*(rc: var RollingCounter, nowMs: int64): float32 =
  rc.tick(nowMs)
  var total: int32 = 0
  for i in 0..<RateBuckets: total += rc.buckets[i]
  float32(total)

# ── Latency histogram (sliding window, percentile queries) ──

const LatencyWindowSize* = 1000

type
  LatencyHistogram* = object
    samples: array[LatencyWindowSize, int64]
    sorted: array[LatencyWindowSize, int64]
    writeIdx: int
    count*: int
    sortDirty: bool
    minVal*, maxVal*: int64

proc init*(lh: var LatencyHistogram) =
  lh.writeIdx = 0
  lh.count = 0
  lh.sortDirty = true
  lh.minVal = int64.high
  lh.maxVal = 0

proc add*(lh: var LatencyHistogram, ns: int64) =
  if ns <= 0: return
  lh.samples[lh.writeIdx] = ns
  lh.writeIdx = (lh.writeIdx + 1) mod LatencyWindowSize
  if lh.count < LatencyWindowSize: lh.count += 1
  lh.sortDirty = true
  if ns < lh.minVal: lh.minVal = ns
  if ns > lh.maxVal: lh.maxVal = ns

proc ensureSorted(lh: var LatencyHistogram) =
  if not lh.sortDirty: return
  for i in 0..<lh.count: lh.sorted[i] = lh.samples[i]
  sort(lh.sorted.toOpenArray(0, lh.count - 1))
  lh.sortDirty = false

proc percentile*(lh: var LatencyHistogram, p: float): int64 =
  if lh.count == 0: return 0
  lh.ensureSorted()
  let idx = min(int(float(lh.count - 1) * p + 0.5), lh.count - 1)
  lh.sorted[idx]

proc p50*(lh: var LatencyHistogram): int64 = lh.percentile(0.50)
proc p95*(lh: var LatencyHistogram): int64 = lh.percentile(0.95)
proc p99*(lh: var LatencyHistogram): int64 = lh.percentile(0.99)
proc p999*(lh: var LatencyHistogram): int64 = lh.percentile(0.999)

# ── Sparkline buffer (circular, 1 sample/sec) ──

type
  SparklineBuffer* = object
    data*: array[SparklineLen, int16]
    writeIdx*: int
    lastUpdateMs*: int64

proc init*(sb: var SparklineBuffer, nowMs: int64) =
  sb.writeIdx = 0
  sb.lastUpdateMs = nowMs
  for i in 0..<SparklineLen: sb.data[i] = 0

proc push*(sb: var SparklineBuffer, value: int16, nowMs: int64) =
  if nowMs - sb.lastUpdateMs < 1000: return
  sb.data[sb.writeIdx] = value
  sb.writeIdx = (sb.writeIdx + 1) mod SparklineLen
  sb.lastUpdateMs = nowMs

proc copyTo*(sb: SparklineBuffer, dest: var array[SparklineLen, int16]) =
  for i in 0..<SparklineLen:
    dest[i] = sb.data[(sb.writeIdx + i) mod SparklineLen]
