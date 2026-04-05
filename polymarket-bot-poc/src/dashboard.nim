# polymarket-bot-poc/src/dashboard.nim — TUI dashboard renderer
#
# Pure ANSI escape code rendering. No external TUI library.
# Panels: header, system, network, latency, queues, feeds,
#         books (up/down), reference, microstructure, trade tape, rates.

import std/[strformat, strutils, times, posix]
import posix/termios as ptermios
import types

# ── ANSI Helpers ──

const
  Reset = "\e[0m"
  Bold = "\e[1m"
  Dim = "\e[2m"
  FgRed = "\e[31m"
  FgGreen = "\e[32m"
  FgYellow = "\e[33m"
  FgCyan {.used.} = "\e[36m"
  FgWhite {.used.} = "\e[37m"
  BgInverse = "\e[7m"

proc moveTo(row, col: int): string = &"\e[{row};{col}H"
proc clearLine(): string = "\e[2K"
proc clearScreen*() = stdout.write("\e[2J\e[H")
proc cursorHome*() = stdout.write("\e[H")
proc hideCursor*() = stdout.write("\e[?25l")
proc showCursor*() = stdout.write("\e[?25h")
proc flushStdout*() = stdout.flushFile()

# ── Color helpers ──

proc colorByThreshold(value, green, yellow: float): string =
  if value <= green: FgGreen
  elif value <= yellow: FgYellow
  else: FgRed

proc feedColor(staleMs: int64): string =
  if staleMs < 100: FgGreen
  elif staleMs < 1000: FgYellow
  else: FgRed

proc queueColor(depth, capacity: int): string =
  let pct = if capacity > 0: depth.float / capacity.float else: 0.0
  if pct < 0.10: FgGreen
  elif pct < 0.50: FgYellow
  else: FgRed

proc dropColor(drops: int64): string =
  if drops == 0: FgGreen
  elif drops <= 100: FgYellow
  else: FgRed

# ── Sparkline rendering ──

const SparkChars: array[9, string] = [" ", "\u2581", "\u2582", "\u2583", "\u2584", "\u2585", "\u2586", "\u2587", "\u2588"]

proc renderSparkline(data: openArray[int16], width: int): string =
  result = ""
  var maxVal: int16 = 1
  let start = max(0, data.len - width)
  for i in start..<data.len:
    if data[i] > maxVal: maxVal = data[i]
  for i in start..<data.len:
    let idx = if maxVal > 0: min(int(data[i].float / maxVal.float * 8), 8) else: 0
    result.add(SparkChars[idx])

# ── BBO bar ──

proc bboBar(bidSize, askSize: float64, width: int = 20): string =
  let total = bidSize + askSize
  if total <= 0: return " ".repeat(width)
  let bidW = int(bidSize / total * width.float)
  let askW = width - bidW
  FgGreen & "\u2588".repeat(bidW) & FgRed & "\u2588".repeat(askW) & Reset

# ── Format helpers ──

proc fmtLat(ns: int64): string =
  if ns <= 0: "---"
  elif ns < 1000: &"{ns}ns"
  elif ns < 1_000_000: &"{ns.float / 1000.0:.1f}us"
  else: &"{ns.float / 1_000_000.0:.1f}ms"

proc fmtBytes(b: int64): string =
  if b < 1024: &"{b}B"
  elif b < 1024 * 1024: &"{b div 1024}KB"
  elif b < 1024 * 1024 * 1024: &"{b div 1024 div 1024}MB"
  else: &"{b.float / (1024*1024*1024).float:.1f}GB"

proc fmtRate(r: float32): string =
  if r < 1000: &"{r:.0f}/s"
  elif r < 1_000_000: &"{r / 1000:.1f}K/s"
  else: &"{r / 1_000_000:.1f}M/s"

proc fmtPrice(p: float64, decimals: int = 3): string =
  if p == 0: "---" else: formatFloat(p, ffDecimal, decimals)

proc fmtComma(p: float64): string =
  let s = &"{p:.2f}"
  let parts = s.split('.')
  var intPart = parts[0]
  let decPart = if parts.len > 1: "." & parts[1] else: ""
  var neg = false
  if intPart.len > 0 and intPart[0] == '-': neg = true; intPart = intPart[1..^1]
  var buf = ""
  for i, c in intPart:
    if i > 0 and (intPart.len - i) mod 3 == 0: buf.add(',')
    buf.add(c)
  if neg: buf = "-" & buf
  buf & decPart

# ── Panel Renderers ──

proc renderHeader*(snap: DashboardSnapshot, row: int): int =
  var line = moveTo(row, 1) & clearLine()
  line.add Bold & " MANTIS "
  for i in 0..<snap.marketCount:
    let label = $snap.markets[i].label
    if i == snap.selectedMarket:
      line.add BgInverse & &" {i+1}:{label} " & Reset & " "
    else:
      line.add Dim & &" {i+1}:{label} " & Reset & " "
  let phStr = snap.phase.phaseStr
  line.add &"{Bold}{phStr}{Reset} {snap.elapsed:+.0f}s"
  # Show selected market's reference price in header
  let sel = snap.selectedMarket
  if sel < snap.marketCount:
    let mkt = snap.markets[sel]
    let refInst = snap.instruments[mkt.refIdx]
    if refInst.mid > 0:
      let refSym = $refInst.symbol  # e.g. "BTCUSDT"
      line.add &"  {FgCyan}{refSym}: ${fmtComma(refInst.mid)}{Reset}"
  stdout.write(line)
  row + 1

proc renderSystemPanel*(snap: DashboardSnapshot, row: int): int =
  stdout.write moveTo(row, 1) & clearLine() & Bold & " SYSTEM" & Reset
  stdout.write moveTo(row+1, 1) & clearLine() &
    &"  CPU: {snap.cpuPercent:.1f}%  THR: {snap.threadCount}  " &
    &"RSS: {fmtBytes(snap.rssBytes)}  VM: {fmtBytes(snap.vmBytes)}"
  row + 2

proc renderNetworkPanel*(snap: DashboardSnapshot, row: int): int =
  stdout.write moveTo(row, 1) & clearLine() & Bold & " NETWORK" & Reset
  let pmRtt = if snap.pmRttUs > 0: &"{snap.pmRttUs.float / 1000:.1f}ms" else: "---"
  let bnRtt = if snap.bnRttUs > 0: &"{snap.bnRttUs.float / 1000:.1f}ms" else: "---"
  stdout.write moveTo(row+1, 1) & clearLine() &
    &"  PM RTT: {pmRtt}  BN RTT: {bnRtt}  " &
    &"PM: {snap.pmBytesPerSec:.0f}B/s  BN: {snap.bnBytesPerSec:.0f}B/s"
  row + 2

proc renderLatencyPanel*(snap: DashboardSnapshot, row: int): int =
  stdout.write moveTo(row, 1) & clearLine() & Bold & " ENGINE LATENCY" & Reset &
    &" (n={snap.latSampleCount})"
  let c99 = colorByThreshold(snap.latP99.float / 1000, 10, 100)
  stdout.write moveTo(row+1, 1) & clearLine() &
    &"  p50: {fmtLat(snap.latP50)}  p95: {fmtLat(snap.latP95)}  " &
    c99 & &"p99: {fmtLat(snap.latP99)}" & Reset &
    &"  p999: {fmtLat(snap.latP999)}"
  stdout.write moveTo(row+2, 1) & clearLine() &
    &"  min: {fmtLat(snap.latMin)}  max: {fmtLat(snap.latMax)}  " &
    Dim & renderSparkline(snap.latSparkline, 30) & Reset
  row + 3

proc renderQueuePanel*(snap: DashboardSnapshot, row: int): int =
  stdout.write moveTo(row, 1) & clearLine() & Bold & " QUEUES" & Reset &
    "  " & Dim & renderSparkline(snap.pmQSparkline, 20) & Reset
  let pmC = queueColor(snap.pmQDepth, RingSize)
  let refC = queueColor(snap.refQDepth, RingSize)
  let telC = queueColor(snap.telemQDepth, RingSize)
  let dC = dropColor(snap.pmQDrops + snap.refQDrops + snap.telemQDrops)
  stdout.write moveTo(row+1, 1) & clearLine() &
    &"  pm: {pmC}{snap.pmQDepth:>5}{Reset}  hi:{snap.pmQHighWater}  " &
    &"ref: {refC}{snap.refQDepth:>5}{Reset}  hi:{snap.refQHighWater}  " &
    &"tel: {telC}{snap.telemQDepth:>5}{Reset}  hi:{snap.telemQHighWater}"
  stdout.write moveTo(row+2, 1) & clearLine() &
    &"  drops: {dC}{snap.pmQDrops}/{snap.refQDrops}/{snap.telemQDrops}{Reset}"
  row + 3

proc renderFeedPanel*(snap: DashboardSnapshot, row: int): int =
  let nowMs = snap.epochMs
  let pmStale = nowMs - snap.pmLastMsgMs
  stdout.write moveTo(row, 1) & clearLine() & Bold & " FEEDS" & Reset
  let pmWsStr = if snap.wsStatePm == 0: FgGreen & "OK" else: FgRed & "DOWN"
  stdout.write moveTo(row+1, 1) & clearLine() &
    &"  PM  {feedColor(pmStale)}\u25CF{Reset} {pmStale}ms  " &
    &"gaps:{snap.pmSeqGaps}  ws:{pmWsStr}{Reset}"
  var bnStale: int64 = 0
  for i in 0..<snap.marketCount:
    let ms = snap.bnLastMsgMs[i]
    if ms > 0:
      let s = nowMs - ms
      if bnStale == 0 or s < bnStale: bnStale = s
  let bnWsStr = if snap.wsStateBn == 0: FgGreen & "OK" else: FgRed & "DOWN"
  stdout.write moveTo(row+2, 1) & clearLine() &
    &"  BN  {feedColor(bnStale)}\u25CF{Reset} {bnStale}ms  " &
    &"gaps:{snap.bnSeqGaps}  ws:{bnWsStr}{Reset}"
  row + 3

proc renderBookPanel*(snap: DashboardSnapshot, inst: InstrumentSnapshot,
                      label: string, row: int): int =
  # Show weighted mid as primary probability (this is what Polymarket displays)
  let probStr = if inst.wmid > 0: &"{inst.wmid * 100:.2f}%" else: "---"
  stdout.write moveTo(row, 1) & clearLine() & Bold & &" {label}" & Reset &
    &"  {FgCyan}{Bold}{probStr}{Reset}"
  let bidStr = fmtPrice(inst.bidPrice)
  let askStr = fmtPrice(inst.askPrice)
  let bar = bboBar(inst.bidSize, inst.askSize, 20)
  stdout.write moveTo(row+1, 1) & clearLine() &
    &"  {FgGreen}{inst.bidSize:>8.0f}{Reset} {bar} " &
    &"{FgGreen}{bidStr}{Reset} | {FgRed}{askStr}{Reset} {FgRed}{inst.askSize:>8.0f}{Reset}"
  let imbC = colorByThreshold(abs(inst.imbalance).float, 0.3, 0.6)
  stdout.write moveTo(row+2, 1) & clearLine() &
    &"  sp:{inst.spread:.3f}  mid:{inst.mid:.4f}  wmid:{inst.wmid:.4f}  " &
    imbC & &"imb:{inst.imbalance:+.2f}" & Reset
  stdout.write moveTo(row+3, 1) & clearLine() &
    &"  lvl:{inst.bidLevels}/{inst.askLevels}  depth:{inst.totalBidDepth:.0f}/{inst.totalAskDepth:.0f}  " &
    &"bbo/s:{inst.bboChangesPerSec:.1f}  rev:{inst.priceReversals}"
  row + 4

proc renderComplementarity*(snap: DashboardSnapshot, row: int): int =
  let sel = snap.selectedMarket
  if sel < snap.marketCount:
    let v = snap.upPlusDown[sel]
    let c = if v >= 0.998 and v <= 1.002: FgGreen
            elif v >= 0.995 and v <= 1.005: FgYellow
            else: FgRed
    stdout.write moveTo(row, 1) & clearLine() &
      &"  up+down: {c}{v:.4f}{Reset}"
  row + 1

proc renderRefPanel*(snap: DashboardSnapshot, refInst: InstrumentSnapshot,
                     row: int): int =
  let refSym = $refInst.symbol  # e.g. "BTCUSDT"
  stdout.write moveTo(row, 1) & clearLine() & Bold &
    &" {refSym}" & Reset & " (Binance)"
  if refInst.mid > 0:
    stdout.write &"  {FgCyan}${fmtComma(refInst.mid)}{Reset}"
  stdout.write moveTo(row+1, 1) & clearLine() &
    &"  bid: {fmtComma(refInst.bidPrice)}  ask: {fmtComma(refInst.askPrice)}  " &
    &"sp:${refInst.spread:.2f}  imb:{refInst.imbalance:+.2f}"
  stdout.write moveTo(row+2, 1) & clearLine() &
    &"  d20<>bbo:{refInst.bboMatchRate:.1f}%  " &
    &"lat:{refInst.avgTradeLatencyMs:.1f}ms"
  row + 3

proc renderMicroPanel*(snap: DashboardSnapshot, inst: InstrumentSnapshot,
                       row: int): int =
  let arrow = if inst.moveDirection > 0: "\u25B2"
              elif inst.moveDirection < 0: "\u25BC" else: "-"
  let runLen = min(abs(inst.consecutiveMoves).int, 5)
  let runStr = arrow.repeat(runLen)
  stdout.write moveTo(row, 1) & clearLine() & Bold & " MICROSTRUCTURE" & Reset
  stdout.write moveTo(row+1, 1) & clearLine() &
    &"  BBO/s:{inst.bboChangesPerSec:.1f}  rev:{inst.priceReversals}  " &
    &"trades/s:{inst.tradesPerSec:.1f}  burst:{inst.burstRate:.0f}"
  let sideC = if inst.lastTradeSide == SideBuy: FgGreen & "B" else: FgRed & "S"
  stdout.write moveTo(row+2, 1) & clearLine() &
    &"  run:{runStr}({inst.consecutiveMoves:+d})  " &
    &"last:{fmtPrice(inst.lastTradePrice)} " & sideC & Reset &
    &" {inst.lastTradeSize:.0f}"
  row + 3

proc renderTradeTape*(snap: DashboardSnapshot, row: int): int =
  stdout.write moveTo(row, 1) & clearLine() & Bold & " TRADE TAPE" & Reset
  for i in 0..<min(MaxTrades.int, 6):
    let idx = (snap.tradeWriteIdx.int - 1 - i + MaxTrades.int) mod MaxTrades.int
    let t = snap.trades[idx]
    if t.epochMs == 0:
      stdout.write moveTo(row+1+i, 1) & clearLine()
      continue
    let ts = fromUnix(t.epochMs div 1000).utc
    let timeStr = ts.format("HH:mm:ss")
    let sideC = if t.side == SideBuy: FgGreen & "BUY " else: FgRed & "SELL"
    stdout.write moveTo(row+1+i, 1) & clearLine() &
      &"  {timeStr}  {sideC}{Reset}  {fmtPrice(t.price)}  ${t.size:.0f}"
  row + 7

proc renderRatePanel*(snap: DashboardSnapshot, row: int): int =
  stdout.write moveTo(row, 1) & clearLine() & Bold & " RATES " & Reset &
    Dim & renderSparkline(snap.rateSparkline, 30) & Reset
  stdout.write moveTo(row+1, 1) & clearLine() &
    &"  pm:{fmtRate(snap.pmEventsPerSec)}  " &
    &"bn<>:{fmtRate(snap.bnBboPerSec)}  " &
    &"bn$:{fmtRate(snap.bnTradePerSec)}  " &
    &"bnd:{fmtRate(snap.bnDepthPerSec)}  " &
    Bold & &"tot:{fmtRate(snap.totalEventsPerSec)}" & Reset
  row + 2

proc renderStatusBar*(snap: DashboardSnapshot, row: int): int =
  stdout.write moveTo(row, 1) & clearLine() &
    Dim & " [1-9]market [q]quit [p]pause [l]latency [t]tape [d]debug [?]help" & Reset
  row + 1

# ── Main render function ──

proc renderDashboard*(snap: DashboardSnapshot) =
  var row = 1
  row = renderHeader(snap, row)
  row += 1
  row = renderSystemPanel(snap, row)
  row = renderNetworkPanel(snap, row)
  row = renderLatencyPanel(snap, row)
  row += 1
  row = renderQueuePanel(snap, row)
  row = renderFeedPanel(snap, row)
  row += 1
  let sel = snap.selectedMarket
  if sel < snap.marketCount:
    let mkt = snap.markets[sel]
    let upInst = snap.instruments[mkt.upIdx]
    let downInst = snap.instruments[mkt.downIdx]
    let refInst = snap.instruments[mkt.refIdx]
    row = renderBookPanel(snap, upInst, $mkt.label & " UP", row)
    row = renderBookPanel(snap, downInst, $mkt.label & " DOWN", row)
    row = renderComplementarity(snap, row)
    row += 1
    row = renderRefPanel(snap, refInst, row)
    row = renderMicroPanel(snap, upInst, row)
  row += 1
  row = renderTradeTape(snap, row)
  row = renderRatePanel(snap, row)
  row += 1
  discard renderStatusBar(snap, row)

# ── Non-blocking keyboard input ──

var origTermios: ptermios.Termios

proc enableRawMode*() =
  discard tcGetAttr(0.cint, addr origTermios)
  var raw = origTermios
  raw.c_lflag = raw.c_lflag and not (ptermios.ECHO or ptermios.ICANON)
  raw.c_cc[ptermios.VMIN] = 0.char
  raw.c_cc[ptermios.VTIME] = 0.char
  discard tcSetAttr(0.cint, ptermios.TCSANOW, addr raw)

proc disableRawMode*() =
  discard tcSetAttr(0.cint, ptermios.TCSANOW, addr origTermios)

proc readKeyNonBlocking*(): char =
  # Use POSIX select with zero timeout instead of poll
  var readfds: TFdSet
  FD_ZERO(readfds)
  FD_SET(0.cint, readfds)
  var timeout: Timeval
  timeout.tv_sec = posix.Time(0)
  timeout.tv_usec = 0
  let ready = select(1.cint, addr readfds, nil, nil, addr timeout)
  if ready > 0 and FD_ISSET(0.cint, readfds) != 0:
    var buf: array[1, char]
    let n = read(0.cint, addr buf[0], 1)
    if n == 1: return buf[0]
  return '\0'
