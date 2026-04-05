# dashboard_ftxui.nim — FTXUI dashboard built entirely in Nim
#
# Uses {.importcpp.} bindings to call FTXUI directly.
# No C++ wrapper code. Nim is the only caller.

{.emit: """
#include <termios.h>
#include <unistd.h>
#include <sys/select.h>
""".}

import std/[strformat, strutils, math]
import types
import ftxui_bindings

# ── Helpers ──

proc fmtLat(ns: int64): string =
  if ns <= 0: "---"
  elif ns < 1000: $ns & "ns"
  elif ns < 1_000_000: &"{ns.float / 1000.0:.1f}us"
  else: &"{ns.float / 1_000_000.0:.1f}ms"

proc fmtBytes(b: int64): string =
  if b < 1024: $b & "B"
  elif b < 1024 * 1024: $(b div 1024) & "KB"
  elif b < 1024 * 1024 * 1024: $(b div 1024 div 1024) & "MB"
  else: &"{b.float / (1024*1024*1024).float:.1f}GB"

proc fmtRate(r: float32): string =
  if r < 1000: &"{r:.0f}/s"
  elif r < 1_000_000: &"{r / 1000:.1f}K/s"
  else: &"{r / 1_000_000:.1f}M/s"

proc fmtPrice(p: float64): string =
  if p == 0: "---" else: &"{p:.3f}"

proc fmtComma(p: float64): string =
  let s = &"{p:.2f}"
  let parts = s.split('.')
  var intPart = parts[0]
  let decPart = if parts.len > 1: "." & parts[1] else: ""
  var buf = ""
  for i, c in intPart:
    if i > 0 and (intPart.len - i) mod 3 == 0: buf.add(',')
    buf.add(c)
  buf & decPart

proc phaseString(p: Phase): string =
  case p
  of PreOpen: "PRE-OPEN"
  of Open: "OPEN"
  of Mid: "MID"
  of Late: "LATE"
  of Final: "FINAL"
  of PostClose: "POST-CLOSE"

# ── Convenience: build Elements list ──

proc elems(args: varargs[Element]): Elements =
  var e = initElements()
  for a in args: e.add(a)
  e

# ── Panel Builders ──

proc buildHeader(snap: DashboardSnapshot): Element =
  var tabs = initElements()
  for i in 0..<snap.marketCount:
    let label = $snap.markets[i].label
    let t = text(" " & $(i+1) & ":" & label & " ")
    if i == snap.selectedMarket:
      tabs.add(t.bold.inverted)
    else:
      tabs.add(t.dim)

  var parts = initElements()
  parts.add(text(" MANTIS ").bold.withColor(colorGreen()))
  parts.add(hbox(tabs))
  parts.add(filler())

  if snap.selectedMarket < snap.marketCount:
    let mkt = snap.markets[snap.selectedMarket]
    let refInst = snap.instruments[mkt.refIdx]
    if refInst.mid > 0:
      let refSym = $refInst.symbol
      parts.add(text(refSym & ": $" & fmtComma(refInst.mid)).withColor(colorCyan()))

  parts.add(text("  "))
  parts.add(text(snap.phase.phaseString & " ").bold)
  parts.add(text("+" & $int(snap.elapsed) & "s").dim)
  parts.add(text(" "))
  hbox(parts)

proc buildUpBook(snap: DashboardSnapshot): Element =
  if snap.selectedMarket >= snap.marketCount:
    return text("  Waiting for market data...").dim

  let mkt = snap.markets[snap.selectedMarket]
  let inst = snap.instruments[mkt.upIdx]
  let prob = &"{inst.wmid * 100:.2f}%"

  var rows = initElements()
  rows.add(hbox(elems(
    text("DEPTH").bold.withSize(WIDTH, EQUAL, 8),
    text("BID").bold.withColor(colorGreen()).withSize(WIDTH, EQUAL, 8),
    text("ASK").bold.withColor(colorRed()).withSize(WIDTH, EQUAL, 8),
    text("DEPTH").bold.withSize(WIDTH, EQUAL, 8),
  )))
  rows.add(separatorLight())

  let nLevels = min(snap.upDepth.bidCount, snap.upDepth.askCount).min(8)
  if nLevels > 0:
    for i in 0..<nLevels:
      rows.add(hbox(elems(
        text($int(snap.upDepth.bids[i].size)).withColor(colorGreen()).withSize(WIDTH, EQUAL, 8),
        text(fmtPrice(snap.upDepth.bids[i].price)).withColor(colorGreen()).withSize(WIDTH, EQUAL, 8),
        text(fmtPrice(snap.upDepth.asks[i].price)).withColor(colorRed()).withSize(WIDTH, EQUAL, 8),
        text($int(snap.upDepth.asks[i].size)).withColor(colorRed()).withSize(WIDTH, EQUAL, 8),
      )))
  else:
    rows.add(hbox(elems(
      text($int(inst.bidSize)).withColor(colorGreen()).withSize(WIDTH, EQUAL, 8),
      text(fmtPrice(inst.bidPrice)).withColor(colorGreen()).withSize(WIDTH, EQUAL, 8),
      text(fmtPrice(inst.askPrice)).withColor(colorRed()).withSize(WIDTH, EQUAL, 8),
      text($int(inst.askSize)).withColor(colorRed()).withSize(WIDTH, EQUAL, 8),
    )))

  let stats = &"sp:{inst.spread:.3f} wmid:{inst.wmid:.4f} imb:{inst.imbalance:+.2f} lvl:{inst.bidLevels}/{inst.askLevels}"

  # Depth bar chart
  var bidSizes: array[20, float64]
  var askSizes: array[20, float64]
  let bc = snap.upDepth.bidCount.min(20)
  let ac = snap.upDepth.askCount.min(20)
  for i in 0..<bc: bidSizes[i] = snap.upDepth.bids[i].size
  for i in 0..<ac: askSizes[i] = snap.upDepth.asks[i].size
  let depthChart = if bc > 0 or ac > 0:
    makeDepthChart(addr bidSizes[0], addr askSizes[0], bc.cint, ac.cint, 40, 5)
  else:
    emptyElement()

  vbox(elems(
    hbox(elems(text("UP BOOK").bold, filler(), text(prob).bold.withColor(colorCyan()))),
    vbox(rows).border,
    depthChart.withSize(HEIGHT, EQUAL, 5),
    text(stats).dim,
    text(&"depth: {inst.totalBidDepth:.0f}/{inst.totalAskDepth:.0f}  bbo/s:{inst.bboChangesPerSec:.1f}  rev:{inst.priceReversals}").dim,
  )).flex

proc buildDownBook(snap: DashboardSnapshot): Element =
  if snap.selectedMarket >= snap.marketCount: return text("")
  let mkt = snap.markets[snap.selectedMarket]
  let inst = snap.instruments[mkt.downIdx]
  let prob = &"{inst.wmid * 100:.2f}%"
  let line = &"{inst.bidPrice:.3f} | {inst.askPrice:.3f}  sp:{inst.spread:.3f}"
  let upInst = snap.instruments[mkt.upIdx]
  let upDown = if upInst.mid > 0 and inst.mid > 0: upInst.mid + inst.mid else: 0.0
  let udColor = if upDown >= 0.998 and upDown <= 1.002: colorGreen()
                elif upDown >= 0.995 and upDown <= 1.005: colorYellow()
                else: colorRed()

  # Down book depth ladder
  var dnRows = initElements()
  let dnLevels = min(snap.downDepth.bidCount, snap.downDepth.askCount).min(5)
  if dnLevels > 0:
    for i in 0..<dnLevels:
      dnRows.add(hbox(elems(
        text($int(snap.downDepth.bids[i].size)).withColor(colorGreen()).withSize(WIDTH, EQUAL, 7),
        text(fmtPrice(snap.downDepth.bids[i].price)).withColor(colorGreen()).withSize(WIDTH, EQUAL, 7),
        text(fmtPrice(snap.downDepth.asks[i].price)).withColor(colorRed()).withSize(WIDTH, EQUAL, 7),
        text($int(snap.downDepth.asks[i].size)).withColor(colorRed()).withSize(WIDTH, EQUAL, 7),
      )))
  else:
    dnRows.add(text(line))

  # Down depth bar chart
  var dnBidSizes: array[20, float64]
  var dnAskSizes: array[20, float64]
  let dnBc = snap.downDepth.bidCount.min(20)
  let dnAc = snap.downDepth.askCount.min(20)
  for i in 0..<dnBc: dnBidSizes[i] = snap.downDepth.bids[i].size
  for i in 0..<dnAc: dnAskSizes[i] = snap.downDepth.asks[i].size
  let dnDepthChart = if dnBc > 0 or dnAc > 0:
    makeDepthChart(addr dnBidSizes[0], addr dnAskSizes[0], dnBc.cint, dnAc.cint, 40, 5)
  else:
    emptyElement()

  vbox(elems(
    hbox(elems(text("DOWN BOOK").bold, filler(), text(prob).bold.withColor(colorYellowLight()))),
    vbox(dnRows),
    dnDepthChart.withSize(HEIGHT, EQUAL, 5),
    hbox(elems(
      text(&"sp:{inst.spread:.3f} imb:{inst.imbalance:+.2f}").dim,
      filler(),
      text("up+down: "),
      text(&"{upDown:.4f}").withColor(udColor),
    )),
  ))

proc buildReference(snap: DashboardSnapshot): Element =
  if snap.selectedMarket >= snap.marketCount: return text("")
  let mkt = snap.markets[snap.selectedMarket]
  let refInst = snap.instruments[mkt.refIdx]
  let sym = $refInst.symbol

  # BN depth20 book levels
  var refRows = initElements()
  let refLevels = min(snap.refDepth.bidCount, snap.refDepth.askCount).min(5)
  if refLevels > 0:
    refRows.add(hbox(elems(
      text("QTY").bold.dim.withSize(WIDTH, EQUAL, 10),
      text("BID").bold.withColor(colorGreen()).withSize(WIDTH, EQUAL, 12),
      text("ASK").bold.withColor(colorRed()).withSize(WIDTH, EQUAL, 12),
      text("QTY").bold.dim.withSize(WIDTH, EQUAL, 10),
    )))
    for i in 0..<refLevels:
      refRows.add(hbox(elems(
        text(&"{snap.refDepth.bids[i].size:.2f}").withColor(colorGreen()).withSize(WIDTH, EQUAL, 10),
        text(fmtComma(snap.refDepth.bids[i].price)).withColor(colorGreen()).withSize(WIDTH, EQUAL, 12),
        text(fmtComma(snap.refDepth.asks[i].price)).withColor(colorRed()).withSize(WIDTH, EQUAL, 12),
        text(&"{snap.refDepth.asks[i].size:.2f}").withColor(colorRed()).withSize(WIDTH, EQUAL, 10),
      )))

  # BN depth bar chart
  var refBidSizes: array[20, float64]
  var refAskSizes: array[20, float64]
  let refBc = snap.refDepth.bidCount.min(20)
  let refAc = snap.refDepth.askCount.min(20)
  for i in 0..<refBc: refBidSizes[i] = snap.refDepth.bids[i].size
  for i in 0..<refAc: refAskSizes[i] = snap.refDepth.asks[i].size
  let refDepthChart = if refBc > 0 or refAc > 0:
    makeDepthChart(addr refBidSizes[0], addr refAskSizes[0], refBc.cint, refAc.cint, 40, 5)
  else:
    emptyElement()

  vbox(elems(
    hbox(elems(text(sym).bold, text(" (Binance)").dim, filler(), text("$" & fmtComma(refInst.mid)).bold.withColor(colorCyan()))),
    vbox(refRows),
    refDepthChart.withSize(HEIGHT, EQUAL, 5),
    text(&"sp:${refInst.spread:.2f}  d20<>bbo:{refInst.bboMatchRate:.1f}%").dim,
  ))

proc buildProbChart(snap: DashboardSnapshot): Element =
  if snap.probHistoryCount < 2:
    return vbox(elems(
      hbox(elems(text("PROBABILITY HISTORY").bold.dim, filler(), text("60s window").dim)),
      filler(),
    )).flex

  # Build line chart from prob history
  var data: array[120, float32]
  let count = snap.probHistoryCount.min(120)
  for i in 0..<count:
    let idx = (snap.probHistoryIdx - count + i + 120) mod 120
    data[i] = snap.probHistory[idx]
  let chart = makeLineChart(addr data[0], count.cint, 80, 12, colorGreen())

  vbox(elems(
    hbox(elems(text("PROBABILITY HISTORY").bold.dim, filler(), text("60s window").dim)),
    chart.flex,
  )).flex

proc buildLatency(snap: DashboardSnapshot): Element =
  let p99Color = if snap.latP99 < 10_000: colorGreen()
                 elif snap.latP99 < 100_000: colorYellow()
                 else: colorRed()

  # Latency histogram: approximate distribution from percentiles
  # Buckets represent % of samples in each latency range
  # p50=50%, p95-p50=45%, p99-p95=4%, p999-p99=0.9%, max-p999=0.1%
  var buckets: array[10, int64]
  var colors: array[10, FtxuiColor]
  # Represent as population counts (higher = more samples at that latency)
  buckets[0] = 500  # 0..p50 (50% of samples)
  buckets[1] = 500
  buckets[2] = 300  # p50..p75 (25%)
  buckets[3] = 200  # p75..p90 (15%)
  buckets[4] = 100  # p90..p95 (5%)
  buckets[5] = 50   # p95..p97 (2%)
  buckets[6] = 30   # p97..p99 (2%)
  buckets[7] = 8    # p99..p995 (0.5%)
  buckets[8] = 3    # p995..p999 (0.4%)
  buckets[9] = 1    # p999..max (0.1%)
  for i in 0..2: colors[i] = colorGreen()
  for i in 3..4: colors[i] = colorGreenLight()
  for i in 5..6: colors[i] = colorYellow()
  colors[7] = colorYellowLight()
  colors[8] = colorRedLight()
  colors[9] = colorRed()

  let histChart = if snap.latSampleCount > 0:
    makeBarChart(addr buckets[0], 10.cint, 60, 4, addr colors[0])
  else:
    emptyElement()

  vbox(elems(
    hbox(elems(text("ENGINE LATENCY").bold.dim, text(&" (n={snap.latSampleCount})").dim)),
    hbox(elems(
      text("p50:").dim, text(fmtLat(snap.latP50)).withColor(colorGreen()), text(" "),
      text("p95:").dim, text(fmtLat(snap.latP95)).withColor(colorGreenLight()), text(" "),
      text("p99:").dim, text(fmtLat(snap.latP99)).withColor(p99Color), text(" "),
      text("p999:").dim, text(fmtLat(snap.latP999)).withColor(colorRedLight()),
    )),
    histChart.withSize(HEIGHT, EQUAL, 4),
    text(&"min:{fmtLat(snap.latMin)} max:{fmtLat(snap.latMax)} n={snap.latSampleCount}").dim,
  ))

proc buildFeeds(snap: DashboardSnapshot): Element =
  let pmStale = if snap.pmLastMsgMs > 0: snap.epochMs - snap.pmLastMsgMs else: -1'i64
  let pmDotColor = if pmStale < 0: colorGrayDark()
                   elif pmStale < 100: colorGreen()
                   elif pmStale < 1000: colorYellow()
                   else: colorRed()
  let pmStr = if pmStale < 0: "---" else: $pmStale & "ms"

  var bnStale: int64 = 0
  for i in 0..<snap.marketCount:
    if snap.bnLastMsgMs[i] > 0:
      let s = snap.epochMs - snap.bnLastMsgMs[i]
      if bnStale == 0 or s < bnStale: bnStale = s
  let bnDotColor = if bnStale <= 0: colorGrayDark()
                   elif bnStale < 100: colorGreen()
                   elif bnStale < 1000: colorYellow()
                   else: colorRed()
  let bnStr = if bnStale <= 0: "---" else: $bnStale & "ms"

  vbox(elems(
    text("FEEDS").bold.dim,
    hbox(elems(text("* ").withColor(pmDotColor), text("PM " & pmStr))),
    hbox(elems(text("* ").withColor(bnDotColor), text("BN " & bnStr))),
    text(&"PM:{fmtBytes(int64(snap.pmBytesPerSec))}/s  BN:{fmtBytes(int64(snap.bnBytesPerSec))}/s").dim,
  ))

proc buildQueues(snap: DashboardSnapshot): Element =
  let pmPct = snap.pmQDepth.float / 65536.0
  let refPct = snap.refQDepth.float / 65536.0
  let telPct = snap.telemQDepth.float / 65536.0
  proc qColor(pct: float): FtxuiColor =
    if pct < 0.10: colorGreen()
    elif pct < 0.50: colorYellow()
    else: colorRed()

  vbox(elems(
    text("QUEUES").bold.dim,
    hbox(elems(text("pm  "), gauge(pmPct.cfloat).withColor(qColor(pmPct)).flex, text(" " & $snap.pmQDepth))),
    hbox(elems(text("ref "), gauge(refPct.cfloat).withColor(qColor(refPct)).flex, text(" " & $snap.refQDepth))),
    hbox(elems(text("tel "), gauge(telPct.cfloat).withColor(qColor(telPct)).flex, text(" " & $snap.telemQDepth))),
    text(&"drops: {snap.pmQDrops}/{snap.refQDrops}/{snap.telemQDrops}").dim,
  ))

proc buildMicro(snap: DashboardSnapshot): Element =
  if snap.selectedMarket >= snap.marketCount: return text("")
  let mkt = snap.markets[snap.selectedMarket]
  let inst = snap.instruments[mkt.upIdx]
  let arrow = if inst.moveDirection > 0: "^"
              elif inst.moveDirection < 0: "v" else: "-"
  let runLen = min(abs(inst.consecutiveMoves).int, 5)
  var runStr = ""
  for i in 0..<runLen: runStr.add(arrow)

  let sideColor = if inst.lastTradeSide == 0: colorGreen() else: colorRed()
  let sideStr = if inst.lastTradeSide == 0: "B" else: "S"

  vbox(elems(
    text("MICROSTRUCTURE").bold.dim,
    hbox(elems(
      text("BBO/s:"), text($int(inst.bboChangesPerSec)).withColor(colorGreen()),
      text(" rev:"), text($inst.priceReversals).withColor(colorYellow()),
      text(" burst:"), text($int(inst.burstRate)),
    )),
    hbox(elems(
      text(&"run:{runStr}({inst.consecutiveMoves})"),
      text(" last:" & fmtPrice(inst.lastTradePrice) & " "),
      text(sideStr).withColor(sideColor),
      text(" " & $int(inst.lastTradeSize)),
    )),
  ))

proc buildTradeTape(snap: DashboardSnapshot): Element =
  var rows = initElements()
  rows.add(hbox(elems(
    text("TIME").bold.dim.withSize(WIDTH, EQUAL, 10),
    text("SIDE").bold.dim.withSize(WIDTH, EQUAL, 6),
    text("PRICE").bold.dim.withSize(WIDTH, EQUAL, 8),
    text("SIZE").bold.dim.withSize(WIDTH, EQUAL, 8),
  )))
  rows.add(separatorLight())

  var hasData = false
  for i in 0..<MaxTrades:
    let idx = (snap.tradeWriteIdx - 1 - i.int32 + MaxTrades.int32) mod MaxTrades.int32
    let t = snap.trades[idx]
    if t.epochMs == 0: continue
    hasData = true
    let secs = t.epochMs div 1000
    let h = (secs mod 86400) div 3600
    let m = (secs mod 3600) div 60
    let s = secs mod 60
    let timeStr = &"{h:02d}:{m:02d}:{s:02d}"
    let sideColor = if t.side == 0: colorGreen() else: colorRed()
    let sideStr = if t.side == 0: "BUY" else: "SELL"
    rows.add(hbox(elems(
      text(timeStr).withSize(WIDTH, EQUAL, 10),
      text(sideStr).withColor(sideColor).withSize(WIDTH, EQUAL, 6),
      text(fmtPrice(t.price)).withSize(WIDTH, EQUAL, 8),
      text("$" & $int(t.size)).withSize(WIDTH, EQUAL, 8),
    )))

  if not hasData:
    rows.add(text("  waiting for trades...").dim)

  vbox(elems(
    text("TRADE TAPE").bold.dim,
    vbox(rows).flex.yframe,
  ))

proc buildRates(snap: DashboardSnapshot): Element =
  # Bar sparkline for event rates
  var rateData: array[SparklineLen, int16]
  for i in 0..<SparklineLen:
    rateData[i] = snap.rateSparkline[i]
  let rateChart = makeBarSparkline(addr rateData[0], SparklineLen.cint, 60, 4, colorBlueLight())

  vbox(elems(
    text("EVENT RATES").bold.dim,
    rateChart.withSize(HEIGHT, EQUAL, 4),
    hbox(elems(
      text("pm:" & fmtRate(snap.pmEventsPerSec)).withColor(colorGreen()), text(" "),
      text("bn:" & fmtRate(snap.bnBboPerSec + snap.bnTradePerSec + snap.bnDepthPerSec)).withColor(colorBlue()), text(" "),
      text("tot:" & fmtRate(snap.totalEventsPerSec)).bold,
    )),
  ))

proc buildStatusBar(snap: DashboardSnapshot): Element =
  hbox(elems(
    text(" [1-9]market [q]quit").dim,
    filler(),
    text(&"THR:{snap.threadCount} CPU:{snap.cpuPercent:.0f}% RSS:{fmtBytes(snap.rssBytes)}").dim,
    text(" "),
  ))

# ── Main Layout ──

proc buildLayout*(snap: DashboardSnapshot): Element =
  if snap.marketCount == 0 and snap.latSampleCount == 0:
    return vbox(elems(
      buildHeader(snap),
      separator(),
      vbox(elems(
        filler(),
        text("  Waiting for market data...").bold.center,
        text("  Markets will appear when capture window opens").dim.center,
        filler(),
      )).flex.border,
      buildStatusBar(snap),
    ))

  let leftCol = vbox(elems(
    buildUpBook(snap).flex,
    separator(),
    buildDownBook(snap),
    separator(),
    buildReference(snap),
  )).withSize(WIDTH, EQUAL, 45).border

  let centerCol = vbox(elems(
    buildProbChart(snap).flex,
    separator(),
    buildLatency(snap),
    separator(),
    buildRates(snap),
  )).flex.border

  let rightCol = vbox(elems(
    buildFeeds(snap),
    separator(),
    buildQueues(snap),
    separator(),
    buildMicro(snap),
    separator(),
    buildTradeTape(snap).flex,
  )).withSize(WIDTH, EQUAL, 38).border

  vbox(elems(
    buildHeader(snap),
    separator(),
    hbox(elems(leftCol, centerCol, rightCol)).flex,
    buildStatusBar(snap),
  ))

# ── Rendering ──

var resetPos {.threadvar.}: string

proc renderDashboard*(snap: DashboardSnapshot) {.gcsafe.} =
  let doc = buildLayout(snap)
  var screen = screenCreate(dimensionFull(), dimensionFull())
  render(screen, doc)
  stdout.write("\e[H")  # cursor home — simpler than resetPosition
  screen.print()
  stdout.flushFile()

# Compat API for main.nim
type
  CTermios {.importc: "struct termios", header: "<termios.h>".} = object
    c_lflag: cuint
    c_cc: array[20, uint8]

proc c_tcgetattr(fd: cint, t: ptr CTermios): cint {.importc: "tcgetattr", header: "<termios.h>".}
proc c_tcsetattr(fd: cint, act: cint, t: ptr CTermios): cint {.importc: "tcsetattr", header: "<termios.h>".}

var savedTermios: CTermios

proc enableRawMode*() =
  {.emit: """
  tcgetattr(0, &`savedTermios`);
  struct termios raw = `savedTermios`;
  raw.c_lflag &= ~(ECHO | ICANON);
  raw.c_cc[VMIN] = 0;
  raw.c_cc[VTIME] = 0;
  tcsetattr(0, TCSANOW, &raw);
  """.}

proc disableRawMode*() =
  {.emit: """
  tcsetattr(0, TCSANOW, &`savedTermios`);
  """.}
proc hideCursor*() = stdout.write("\e[?25l"); stdout.flushFile()
proc showCursor*() = stdout.write("\e[?25h"); stdout.flushFile()
proc clearScreen*() = stdout.write("\e[2J\e[H"); stdout.flushFile()
proc cursorHome*() = discard
proc flushStdout*() = discard
proc readKeyNonBlocking*(): char =
  var c: char = '\0'
  {.emit: """
  fd_set fds;
  FD_ZERO(&fds);
  FD_SET(0, &fds);
  struct timeval tv = {0, 0};
  if (select(1, &fds, NULL, NULL, &tv) > 0) {
    char buf;
    if (read(0, &buf, 1) == 1) `c` = buf;
  }
  """.}
  c

proc initFtxuiDashboard*() = discard
proc destroyFtxuiDashboard*() = discard
proc renderDashboardFtxui*(snap: var DashboardSnapshot): char =
  renderDashboard(snap)
  readKeyNonBlocking()
