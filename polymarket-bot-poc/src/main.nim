# polymarket-bot-poc/src/main.nim — 6-thread capture pipeline with TUI dashboard
#
# Thread model:
#   Main Thread  — market discovery, lifecycle, waits for threads
#   PM Ingest    — Polymarket WS -> pm_q (FeedEvent)
#   BN Ingest    — Binance WS (3 feeds) -> ref_q (FeedEvent)
#   Engine       — busy-spin consumer of pm_q + ref_q -> telem_q (TelemetryEvent)
#   Telemetry    — consumer of telem_q -> tape files + dash_q (DashboardSnapshot)
#   Dashboard    — consumer of dash_q -> ANSI TUI rendering
#
# Compile: cd polymarket-bot-poc && nim c src/main.nim

import std/[asyncdispatch, atomics, httpclient, json, monotimes, net,
            os, algorithm, sequtils, strformat, strutils, tables, times,
            parseopt]
import ws
import types, spsc, engine_book, stats, system_metrics, dashboard, tape_format
import constantine/threadpool/crossthread/backoff  # Eventcount

# ═══════════════════════════════════════════════════════════════════════════
#  CONSTANTS
# ═══════════════════════════════════════════════════════════════════════════

const
  GammaApi = "https://gamma-api.polymarket.com"
  WsMarketUrl = "wss://ws-subscriptions-clob.polymarket.com/ws/market"
  BnBookTickerUrl = "wss://stream.binance.com:9443/ws/btcusdt@bookTicker"
  BnTradeUrl = "wss://stream.binance.com:9443/ws/btcusdt@trade"
  BnDepth20Url = "wss://stream.binance.com:9443/ws/btcusdt@depth20@100ms"
  BnRestBbo = "https://api.binance.com/api/v3/ticker/bookTicker?symbol=BTCUSDT"

  # Instrument IDs (no strings in hot path)
  InstUp: uint32 = 0
  InstDown: uint32 = 1
  InstRef: uint32 = 0xFFFF

# ═══════════════════════════════════════════════════════════════════════════
#  PM INGEST THREAD
# ═══════════════════════════════════════════════════════════════════════════

proc pmIngestThread(ss: ptr SharedState) {.thread.} =
  let pmQ = cast[ptr SpscRing[FeedEvent]](ss.pmQ)

  proc run() {.async.} =
    let tokenUp = $ss.registry.markets[0].tokenUp
    let tokenDown = $ss.registry.markets[0].tokenDown
    let pmWs = await newWebSocket(WsMarketUrl)
    defer: pmWs.close()

    await pmWs.send($ %*{
      "assets_ids": [tokenUp, tokenDown],
      "type": "market", "custom_feature_enabled": true
    })

    var lastPing = epochTime()
    var pmSeqNo: uint32 = 0

    while ss.running.load(moRelaxed) and epochTime() < ss.captureEnd.float:
      # Keepalive
      if epochTime() - lastPing > PingIntervalSec:
        try: await pmWs.send("PING") except: break
        lastPing = epochTime()

      var raw: string
      try: raw = await pmWs.receiveStrPacket()
      except WebSocketClosedError: break
      except: continue
      if raw.len == 0 or raw == "PONG": continue

      # Network byte counter
      discard ss.pmBytesTotal.fetchAdd(int64(raw.len), moRelaxed)
      ss.pmLastMsgNs.store(getMonoTime().ticks, moRelaxed)

      var parsed: JsonNode
      try: parsed = parseJson(raw) except: continue

      let ts = epochTime()
      let ns = nowNs(ss.monoBase)
      let epochMs = int64(ts * 1000)
      let msgs = if parsed.kind == JArray: parsed.elems else: @[parsed]

      for msg in msgs:
        if msg.kind != JObject: continue
        let et = msg.getOrDefault("event_type").getStr("")
        let aid = msg.getOrDefault("asset_id").getStr("")
        let isUp = aid == tokenUp
        let isDown = aid == tokenDown
        if not isUp and not isDown and et != "price_change": continue
        let instId = if isUp: InstUp elif isDown: InstDown else: InstUp

        if et == "book":
          pmSeqNo += 1
          discard pmQ.tryPush(FeedEvent(
            kind: ekPmBookClear, instrumentId: instId,
            localNs: ns, localEpochMs: epochMs, seqNo: pmSeqNo))
          let bidsNode = msg.getOrDefault("bids")
          let asksNode = msg.getOrDefault("asks")
          let bidCount = if bidsNode.kind == JArray: bidsNode.len else: 0
          let askCount = if asksNode.kind == JArray: asksNode.len else: 0
          for i, item in bidsNode.elems:
            let p = item["price"].getStr
            let pm = int16(parseFloat(p) * 1000.0 + 0.5)
            let isLast = (i == bidCount - 1 and askCount == 0)
            pmSeqNo += 1
            discard pmQ.tryPush(FeedEvent(
              kind: ekPmDelta, instrumentId: instId,
              localNs: ns, localEpochMs: epochMs,
              price: parseFloat(p), size: parseFloat(item["size"].getStr),
              side: SideBuy, priceMilli: pm, seqNo: pmSeqNo,
              flags: (if isLast: FlagLastInBatch else: 0)))
          for i, item in asksNode.elems:
            let p = item["price"].getStr
            let pm = int16(parseFloat(p) * 1000.0 + 0.5)
            let isLast = (i == askCount - 1)
            pmSeqNo += 1
            discard pmQ.tryPush(FeedEvent(
              kind: ekPmDelta, instrumentId: instId,
              localNs: ns, localEpochMs: epochMs,
              price: parseFloat(p), size: parseFloat(item["size"].getStr),
              side: SideSell, priceMilli: pm, seqNo: pmSeqNo,
              flags: (if isLast: FlagLastInBatch else: 0)))

        elif et == "price_change":
          var relevantChanges: seq[JsonNode] = @[]
          for ch in msg.getOrDefault("price_changes"):
            let caid = ch.getOrDefault("asset_id").getStr("")
            if caid == tokenUp or caid == tokenDown:
              relevantChanges.add(ch)
          for i, ch in relevantChanges:
            let caid = ch.getOrDefault("asset_id").getStr("")
            let cInstId = if caid == tokenUp: InstUp else: InstDown
            let p = ch["price"].getStr
            let pm = int16(parseFloat(p) * 1000.0 + 0.5)
            let isLast = (i == relevantChanges.len - 1)
            pmSeqNo += 1
            discard pmQ.tryPush(FeedEvent(
              kind: ekPmDelta, instrumentId: cInstId,
              localNs: ns, localEpochMs: epochMs,
              price: parseFloat(p), size: parseFloat(ch["size"].getStr),
              side: (if ch["side"].getStr == "BUY": SideBuy else: SideSell),
              priceMilli: pm, seqNo: pmSeqNo,
              flags: (if isLast: FlagLastInBatch else: 0)))

        elif et == "last_trade_price" and (isUp or isDown):
          pmSeqNo += 1
          discard pmQ.tryPush(FeedEvent(
            kind: ekPmTrade, instrumentId: instId,
            localNs: ns, localEpochMs: epochMs, seqNo: pmSeqNo,
            price: parseFloat(msg.getOrDefault("price").getStr("0")),
            size: parseFloat(msg.getOrDefault("size").getStr("0")),
            side: (if msg.getOrDefault("side").getStr == "BUY": SideBuy else: SideSell)))

  try: waitFor run()
  except Exception as e: echo "  [pm_ingest] error: " & e.msg

# ═══════════════════════════════════════════════════════════════════════════
#  BN INGEST THREAD
# ═══════════════════════════════════════════════════════════════════════════

proc bnIngestThread(ss: ptr SharedState) {.thread.} =
  let refQ = cast[ptr SpscRing[FeedEvent]](ss.refQ)

  proc run() {.async.} =
    proc bboFeed() {.async.} =
      var bboWs: WebSocket
      try: bboWs = await newWebSocket(BnBookTickerUrl)
      except:
        let client = newAsyncHttpClient()
        defer: client.close()
        while ss.running.load(moRelaxed) and epochTime() < ss.captureEnd.float:
          try:
            let resp = await client.getContent(BnRestBbo)
            discard ss.bnBytesTotal.fetchAdd(int64(resp.len), moRelaxed)
            let msg = parseJson(resp)
            let ns = nowNs(ss.monoBase)
            discard refQ.tryPush(FeedEvent(
              kind: ekBnBbo, instrumentId: InstRef,
              localNs: ns, localEpochMs: int64(epochTime() * 1000),
              bnBid: parseFloat(msg["bidPrice"].getStr),
              bnAsk: parseFloat(msg["askPrice"].getStr),
              bnBidQty: parseFloat(msg["bidQty"].getStr),
              bnAskQty: parseFloat(msg["askQty"].getStr)))
          except: discard
          await sleepAsync(200)
        return
      defer: bboWs.close()
      while ss.running.load(moRelaxed) and epochTime() < ss.captureEnd.float:
        var raw: string
        try: raw = await bboWs.receiveStrPacket()
        except WebSocketClosedError: break
        except: continue
        discard ss.bnBytesTotal.fetchAdd(int64(raw.len), moRelaxed)
        ss.bnLastMsgNs[0].store(getMonoTime().ticks, moRelaxed)
        var msg: JsonNode
        try: msg = parseJson(raw) except: continue
        let ns = nowNs(ss.monoBase)
        discard refQ.tryPush(FeedEvent(
          kind: ekBnBbo, instrumentId: InstRef,
          localNs: ns, localEpochMs: int64(epochTime() * 1000),
          bnBid: parseFloat(msg.getOrDefault("b").getStr("0")),
          bnAsk: parseFloat(msg.getOrDefault("a").getStr("0")),
          bnBidQty: parseFloat(msg.getOrDefault("B").getStr("0")),
          bnAskQty: parseFloat(msg.getOrDefault("A").getStr("0")),
          bnUpdateId: msg.getOrDefault("u").getBiggestInt(0)))

    proc tradeFeed() {.async.} =
      var tradeWs: WebSocket
      try: tradeWs = await newWebSocket(BnTradeUrl)
      except Exception as e:
        echo "  [bn_trade] WS failed: " & e.msg; return
      defer: tradeWs.close()
      while ss.running.load(moRelaxed) and epochTime() < ss.captureEnd.float:
        var raw: string
        try: raw = await tradeWs.receiveStrPacket()
        except WebSocketClosedError: break
        except: continue
        discard ss.bnBytesTotal.fetchAdd(int64(raw.len), moRelaxed)
        ss.bnLastMsgNs[0].store(getMonoTime().ticks, moRelaxed)
        var msg: JsonNode
        try: msg = parseJson(raw) except: continue
        let ns = nowNs(ss.monoBase)
        discard refQ.tryPush(FeedEvent(
          kind: ekBnTrade, instrumentId: InstRef,
          localNs: ns, localEpochMs: int64(epochTime() * 1000),
          price: parseFloat(msg.getOrDefault("p").getStr("0")),
          size: parseFloat(msg.getOrDefault("q").getStr("0")),
          bnEventTimeMs: msg.getOrDefault("E").getBiggestInt(0),
          bnTradeTimeMs: msg.getOrDefault("T").getBiggestInt(0),
          bnIsBuyerMaker: msg.getOrDefault("m").getBool(false)))

    var bnDepthSeqNo: uint32 = 0

    proc depthFeed() {.async.} =
      var depthWs: WebSocket
      try: depthWs = await newWebSocket(BnDepth20Url)
      except Exception as e:
        echo "  [bn_depth20] WS failed: " & e.msg; return
      defer: depthWs.close()
      while ss.running.load(moRelaxed) and epochTime() < ss.captureEnd.float:
        var raw: string
        try: raw = await depthWs.receiveStrPacket()
        except WebSocketClosedError: break
        except: continue
        discard ss.bnBytesTotal.fetchAdd(int64(raw.len), moRelaxed)
        ss.bnLastMsgNs[0].store(getMonoTime().ticks, moRelaxed)
        var msg: JsonNode
        try: msg = parseJson(raw) except: continue
        let ns = nowNs(ss.monoBase)
        let epochMs = int64(epochTime() * 1000)
        let lastUpdateId = msg.getOrDefault("lastUpdateId").getBiggestInt(0)
        let bidsNode = msg.getOrDefault("bids")
        let asksNode = msg.getOrDefault("asks")
        let bidCount = if bidsNode.kind == JArray: bidsNode.len else: 0
        let askCount = if asksNode.kind == JArray: asksNode.len else: 0

        bnDepthSeqNo += 1
        discard refQ.tryPush(FeedEvent(
          kind: ekPmBookClear, instrumentId: InstRef,
          localNs: ns, localEpochMs: epochMs, seqNo: bnDepthSeqNo,
          bnUpdateId: lastUpdateId))

        for i in 0..<bidCount:
          let level = bidsNode[i]
          let price = parseFloat(level[0].getStr("0"))
          let qty = parseFloat(level[1].getStr("0"))
          let isLast = (i == bidCount - 1 and askCount == 0)
          bnDepthSeqNo += 1
          discard refQ.tryPush(FeedEvent(
            kind: ekBnDepth, instrumentId: InstRef,
            localNs: ns, localEpochMs: epochMs,
            price: price, size: qty, side: SideBuy,
            flags: (if isLast: FlagLastInBatch else: 0),
            seqNo: bnDepthSeqNo,
            bnUpdateId: lastUpdateId,
            bnDepthBidLevels: int16(bidCount),
            bnDepthAskLevels: int16(askCount)))
        for i in 0..<askCount:
          let level = asksNode[i]
          let price = parseFloat(level[0].getStr("0"))
          let qty = parseFloat(level[1].getStr("0"))
          let isLast = (i == askCount - 1)
          bnDepthSeqNo += 1
          discard refQ.tryPush(FeedEvent(
            kind: ekBnDepth, instrumentId: InstRef,
            localNs: ns, localEpochMs: epochMs,
            price: price, size: qty, side: SideSell,
            flags: (if isLast: FlagLastInBatch else: 0),
            seqNo: bnDepthSeqNo,
            bnUpdateId: lastUpdateId,
            bnDepthBidLevels: int16(bidCount),
            bnDepthAskLevels: int16(askCount)))

    let f1 = bboFeed()
    let f2 = tradeFeed()
    let f3 = depthFeed()
    while ss.running.load(moRelaxed) and epochTime() < ss.captureEnd.float:
      await sleepAsync(200)

  try: waitFor run()
  except Exception as e: echo "  [bn_ingest] error: " & e.msg

# ═══════════════════════════════════════════════════════════════════════════
#  ENGINE THREAD — busy-spin, owner thread, no allocations
# ═══════════════════════════════════════════════════════════════════════════

proc engineThread(ss: ptr SharedState) {.thread.} =
  let pmQ = cast[ptr SpscRing[FeedEvent]](ss.pmQ)
  let refQ = cast[ptr SpscRing[FeedEvent]](ss.refQ)
  let telemQ = cast[ptr SpscRing[TelemetryEvent]](ss.telemQ)

  var books: array[MaxInstruments, EngineBook]
  var refBooks: array[MaxInstruments, BnBook]
  var btcMid, btcBid, btcAsk = 0.0
  var lastBbo: array[MaxInstruments, tuple[bid, ask: float64]]
  var lastBboMid: array[MaxInstruments, float64]
  var lastBboDir: array[MaxInstruments, int]
  var bboChanges, priceReversals = 0
  var downBboChanges = 0
  var engineEvCount = 0

  # BN book cross-validation counters
  var bnBookUpdates = 0
  var bnBboMatches = 0
  var bnBboMismatches = 0

  # Snapshot batching
  var snapshotNs: array[MaxInstruments, int64]

  proc bookValid(instIdx: int): bool =
    let (bp, _) = books[instIdx].bids.bestPriceF(true)
    let (ap, _) = books[instIdx].asks.bestPriceF(false)
    bp > 0 and ap > 0 and ap > bp

  template emitTelemetry(ev: FeedEvent, telKind: TelemetryKind, latNs: int64) =
    let idx = ev.instrumentId.int
    let bookIdx = if idx < MaxInstruments: idx else: 0
    let (bp, bs) = books[bookIdx].bids.bestPriceF(true)
    let (ap, az) = books[bookIdx].asks.bestPriceF(false)
    let um = if bp > 0 and ap > 0: (bp + ap) / 2.0 else: 0.0
    let us = if bp > 0 and ap > 0: ap - bp else: 0.0
    let wm = books[bookIdx].weightedMid()
    let elapsed = float(ev.localEpochMs - int64(ss.windowStart) * 1000) / 1000.0
    engineEvCount += 1
    discard telemQ.tryPush(TelemetryEvent(
      kind: telKind,
      phase: getPhase(elapsed, ss.duration),
      localNs: ev.localNs, localEpochMs: ev.localEpochMs,
      elapsed: elapsed,
      bidPrice: bp, askPrice: ap,
      bidSize: bs, askSize: az,
      mid: um, spread: us, weightedMid: wm,
      bidLevels: int32(books[bookIdx].bidLevelCount()),
      askLevels: int32(books[bookIdx].askLevelCount()),
      totalBidDepth: books[bookIdx].totalBidDepth(),
      totalAskDepth: books[bookIdx].totalAskDepth(),
      btcMid: btcMid, btcBid: btcBid, btcAsk: btcAsk,
      evKind: ev.kind, instId: ev.instrumentId,
      tradePrice: ev.price, tradeSize: ev.size,
      tradeSide: ev.side,
      flags: ev.flags, seqNo: ev.seqNo,
      bnEventTimeMs: ev.bnEventTimeMs,
      bnTradeTimeMs: ev.bnTradeTimeMs,
      bnBidQty: ev.bnBidQty, bnAskQty: ev.bnAskQty,
      engineLatencyNs: latNs))

  # ── Busy-spin event loop ──
  var ev: FeedEvent
  while ss.running.load(moRelaxed):
    var processed = false

    # Priority: PM events first (market data), then reference
    while pmQ.tryPop(ev):
      processed = true
      let t0 = getMonoTime().ticks
      if ev.kind == ekShutdown: ss.running.store(false, moRelaxed); break

      let instIdx = ev.instrumentId.int
      if instIdx >= MaxInstruments: continue

      case ev.kind
      of ekPmBookClear:
        books[instIdx].bids.clearSide()
        books[instIdx].asks.clearSide()
        snapshotNs[instIdx] = ev.localNs

      of ekPmDelta:
        if ev.side == SideBuy:
          books[instIdx].bids.applyLevel(ev.priceMilli, ev.size, true)
        else:
          books[instIdx].asks.applyLevel(ev.priceMilli, ev.size, false)
        books[instIdx].changeCount += 1

        let inSnapshot = snapshotNs[instIdx] != 0
        if inSnapshot and ev.localNs != snapshotNs[instIdx]:
          snapshotNs[instIdx] = 0

        if snapshotNs[instIdx] != 0:
          discard  # still in snapshot, suppress telemetry
        else:
          # BBO tracking
          if instIdx < MaxInstruments and bookValid(instIdx):
            let (bp, _) = books[instIdx].bids.bestPriceF(true)
            let (ap, _) = books[instIdx].asks.bestPriceF(false)
            let curBbo = (bp, ap)
            if lastBbo[instIdx][0] > 0 and (curBbo[0] != lastBbo[instIdx][0] or curBbo[1] != lastBbo[instIdx][1]):
              bboChanges += 1
              let newMid = (bp + ap) / 2.0
              if lastBboMid[instIdx] > 0:
                let dir = if newMid > lastBboMid[instIdx]: 1
                          elif newMid < lastBboMid[instIdx]: -1 else: 0
                if dir != 0 and lastBboDir[instIdx] != 0 and dir != lastBboDir[instIdx]:
                  priceReversals += 1
                if dir != 0: lastBboDir[instIdx] = dir
              lastBboMid[instIdx] = newMid
              let latNs = getMonoTime().ticks - t0
              emitTelemetry(ev, tkTopOfBook, latNs)
            lastBbo[instIdx] = (bp, ap)

          let latNs = getMonoTime().ticks - t0
          emitTelemetry(ev, tkBookUpdate, latNs)

      of ekPmTrade:
        let latNs = getMonoTime().ticks - t0
        emitTelemetry(ev, tkTrade, latNs)

      else: discard

    # Reference feed
    while refQ.tryPop(ev):
      processed = true
      let t0 = getMonoTime().ticks
      case ev.kind
      of ekBnBbo:
        if ev.bnBid > 0 and ev.bnAsk > 0:
          btcBid = ev.bnBid; btcAsk = ev.bnAsk
          btcMid = (btcBid + btcAsk) / 2.0
        let latNs = getMonoTime().ticks - t0
        emitTelemetry(ev, tkBnBbo, latNs)

      of ekBnTrade:
        let latNs = getMonoTime().ticks - t0
        emitTelemetry(ev, tkBnTrade, latNs)

      of ekPmBookClear:
        if ev.instrumentId == InstRef:
          refBooks[0].bidCount = 0
          refBooks[0].askCount = 0
          refBooks[0].lastUpdateId = ev.bnUpdateId
          let latNs = getMonoTime().ticks - t0
          emitTelemetry(ev, tkBnDepth, latNs)

      of ekBnDepth:
        if ev.side == SideBuy and refBooks[0].bidCount < BnDepthLevels:
          refBooks[0].bids[refBooks[0].bidCount] = BnBookLevel(price: ev.price, qty: ev.size)
          refBooks[0].bidCount += 1
        elif ev.side == SideSell and refBooks[0].askCount < BnDepthLevels:
          refBooks[0].asks[refBooks[0].askCount] = BnBookLevel(price: ev.price, qty: ev.size)
          refBooks[0].askCount += 1

        if (ev.flags and FlagLastInBatch) != 0:
          refBooks[0].valid = true
          bnBookUpdates += 1
          let (rbid, _) = refBooks[0].bnBestBid()
          let (rask, _) = refBooks[0].bnBestAsk()
          if rbid > 0 and rask > 0 and btcBid > 0 and btcAsk > 0:
            if abs(rbid - btcBid) <= 0.021 and abs(rask - btcAsk) <= 0.021:
              bnBboMatches += 1
            else:
              bnBboMismatches += 1

        let latNs = getMonoTime().ticks - t0
        emitTelemetry(ev, tkBnDepth, latNs)

      else: discard

    if not processed:
      cpuRelax()

  # Write final counters
  ss.summary.bboChanges = bboChanges
  ss.summary.priceReversals = priceReversals
  ss.summary.bnBookUpdates = bnBookUpdates
  ss.summary.bnBboMatches = bnBboMatches
  ss.summary.bnBboMismatches = bnBboMismatches

# ═══════════════════════════════════════════════════════════════════════════
#  TELEMETRY THREAD — tape writer, analytics, dashboard snapshot builder
# ═══════════════════════════════════════════════════════════════════════════

proc telemetryThread(ss: ptr SharedState) {.thread.} =
  let telemQ = cast[ptr SpscRing[TelemetryEvent]](ss.telemQ)
  let dashQ = cast[ptr SmallSpscRing[DashboardSnapshot]](ss.dashQ)
  let baseName = $ss.tapeDir / "tape_" & $ss.registry.markets[0].slug

  # ── Binary tapes ──
  let inputTapeHeader = TapeHeader(
    magic: TapeMagic, version: TapeVersion,
    recordSize: InputRecordSize.uint32,
    startTs: uint64(ss.windowStart) * 1_000_000_000,
    instrumentCount: 3,
  )
  let stateTapeHeader = TapeHeader(
    magic: TapeMagic, version: TapeVersion,
    recordSize: StateRecordSize.uint32,
    startTs: uint64(ss.windowStart) * 1_000_000_000,
    instrumentCount: 3,
  )
  var inputTape = initMmapTapeWriter(baseName & ".input.bin", inputTapeHeader)
  var stateTape = initMmapTapeWriter(baseName & ".state.bin", stateTapeHeader,
                                      capacity = 4 * 1024 * 1024)
  defer:
    inputTape.finalize()
    stateTape.finalize()

  var globalSeq: uint64 = 0
  var count = 0
  var pmEvents, bnBboEvents, bnTradeEvents, bnDepthEvents, pmTrades = 0
  var btcOpen, lastBtcMid = 0.0
  var probPeak, maxDD = 0.0
  var lastUpMid = 0.0
  var largestTrade = 0.0
  var lastTradeMs: int64 = 0
  var interTradeTimes: seq[float] = @[]
  var tradeTimestamps: seq[float] = @[]
  var bnTradeLatencies: seq[float] = @[]
  var bnSpreads: seq[float] = @[]
  var bnPriceSteps: seq[float] = @[]
  var lastBnPrice = 0.0

  # Per-instrument state for dashboard
  var instBidPrice, instAskPrice: array[MaxInstruments, float64]
  var instBidSize, instAskSize: array[MaxInstruments, float64]
  var instMid, instSpread, instWmid: array[MaxInstruments, float64]
  var instBidLevels, instAskLevels: array[MaxInstruments, int32]
  var instTotalBidDepth, instTotalAskDepth: array[MaxInstruments, float64]
  var instTradeCount: array[MaxInstruments, int32]
  var instLastTradePrice: array[MaxInstruments, float64]
  var instLastTradeSide: array[MaxInstruments, uint8]
  var instLastTradeSize: array[MaxInstruments, float64]

  # Rolling counters
  var totalRate, pmRate, bnBboRate, bnTradeRate, bnDepthRate: RollingCounter
  var instBboRate: array[MaxInstruments, RollingCounter]
  var instTradeRate: array[MaxInstruments, RollingCounter]
  let initMs = int64(epochTime() * 1000)
  totalRate.init(initMs); pmRate.init(initMs)
  bnBboRate.init(initMs); bnTradeRate.init(initMs); bnDepthRate.init(initMs)
  for i in 0..<MaxInstruments:
    instBboRate[i].init(initMs)
    instTradeRate[i].init(initMs)

  # Latency histogram
  var latHist: LatencyHistogram
  latHist.init()

  # Sparklines
  var pmQSpark, refQSpark, latSpark, rateSpark: SparklineBuffer
  pmQSpark.init(initMs); refQSpark.init(initMs)
  latSpark.init(initMs); rateSpark.init(initMs)

  # System metrics
  var sysMet: SystemMetrics
  sysMet.init(6)  # 6 threads
  var lastSysMs: int64 = initMs

  # Byte counters for rate
  var lastPmBytes, lastBnBytes: int64
  var lastByteMs: int64 = initMs

  # Dashboard push timing
  var lastDashPushMs: int64 = initMs

  var ev: TelemetryEvent
  var spinCount = 0
  while ss.running.load(moRelaxed) or telemQ.len > 0:
    if not telemQ.tryPop(ev):
      spinCount += 1
      if spinCount > 64:
        cpuRelax()
        spinCount = 0
      # Still build dashboard snapshots even when idle
      let wallMs = int64(epochTime() * 1000)
      if wallMs - lastDashPushMs >= 100:
        lastDashPushMs = wallMs
        # Build and push snapshot (below, after the event processing block)
        # We need to jump to the snapshot builder - use a flag
      continue
    spinCount = 0
    count += 1

    let wallMs = int64(epochTime() * 1000)

    # ── Binary input tape ──
    globalSeq += 1
    inputTape.appendInput(InputRecord(
      kind: ev.evKind.uint8,
      instrumentId: (if ev.instId == InstUp: 0'u8 elif ev.instId == InstDown: 1'u8 else: 2'u8),
      side: ev.tradeSide,
      flags: ev.flags,
      seqNo: ev.seqNo,
      wallNs: ev.localNs,
      epochMs: ev.localEpochMs,
      price: ev.tradePrice,
      size: ev.tradeSize,
      priceMilli: (if ev.tradePrice > 0 and ev.tradePrice < 1: int16(ev.tradePrice * 1000 + 0.5) else: 0),
      bnEventTimeMs: ev.bnEventTimeMs,
      bnTradeTimeMs: ev.bnTradeTimeMs,
      bnBid: ev.btcBid,
      bnAsk: ev.btcAsk,
      bnBidQty: ev.bnBidQty,
      bnAskQty: ev.bnAskQty,
      globalSeq: globalSeq,
    ))

    # ── Binary state tape ──
    if ev.kind == tkTopOfBook and ev.bidPrice > 0:
      let imb5 = if ev.bidSize + ev.askSize > 0:
                   float32((ev.bidSize - ev.askSize) / (ev.bidSize + ev.askSize))
                 else: 0'f32
      stateTape.appendState(StateRecord(
        bidPrice: ev.bidPrice,
        askPrice: ev.askPrice,
        bidSize: ev.bidSize,
        askSize: ev.askSize,
        microPrice: ev.weightedMid,
        spread: ev.spread,
        instrumentId: (if ev.instId == InstUp: 0'u8 elif ev.instId == InstDown: 1'u8 else: 2'u8),
        phase: ev.phase.uint8,
        seqNo: ev.seqNo,
        wallNs: ev.localNs,
        epochMs: ev.localEpochMs,
        imbalance5: imb5,
        bidLevels: ev.bidLevels.uint16,
        askLevels: ev.askLevels.uint16,
        btcMid: ev.btcMid,
        globalSeq: globalSeq,
      ))

    # ── Engine latency ──
    if ev.engineLatencyNs > 0:
      latHist.add(ev.engineLatencyNs)

    # ── Per-instrument state update ──
    let iid = ev.instId.int
    if iid < MaxInstruments:
      instBidPrice[iid] = ev.bidPrice
      instAskPrice[iid] = ev.askPrice
      instBidSize[iid] = ev.bidSize
      instAskSize[iid] = ev.askSize
      instMid[iid] = ev.mid
      instSpread[iid] = ev.spread
      instWmid[iid] = ev.weightedMid
      instBidLevels[iid] = ev.bidLevels
      instAskLevels[iid] = ev.askLevels
      instTotalBidDepth[iid] = ev.totalBidDepth
      instTotalAskDepth[iid] = ev.totalAskDepth

    # ── Accumulate stats ──
    case ev.kind
    of tkBookUpdate:
      pmEvents += 1; pmRate.increment(wallMs)
      totalRate.increment(wallMs)
      if iid < MaxInstruments:
        instBboRate[iid].increment(wallMs)
      if ev.mid > 0:
        lastUpMid = ev.mid
        if ev.mid > probPeak: probPeak = ev.mid
        let dd = probPeak - ev.mid
        if dd > maxDD: maxDD = dd

    of tkTrade:
      pmEvents += 1; pmTrades += 1; pmRate.increment(wallMs)
      totalRate.increment(wallMs)
      if ev.tradeSize > largestTrade: largestTrade = ev.tradeSize
      tradeTimestamps.add(ev.elapsed)
      if lastTradeMs > 0:
        interTradeTimes.add(float(ev.localEpochMs - lastTradeMs))
      lastTradeMs = ev.localEpochMs
      if iid < MaxInstruments:
        instTradeCount[iid] += 1
        instTradeRate[iid].increment(wallMs)
        instLastTradePrice[iid] = ev.tradePrice
        instLastTradeSide[iid] = ev.tradeSide
        instLastTradeSize[iid] = ev.tradeSize

    of tkBnBbo:
      bnBboEvents += 1; bnBboRate.increment(wallMs)
      totalRate.increment(wallMs)
      if ev.btcMid > 0:
        lastBtcMid = ev.btcMid
        if btcOpen == 0 and ev.elapsed >= 0:
          btcOpen = ev.btcMid
      if ev.btcBid > 0 and ev.btcAsk > 0:
        bnSpreads.add(ev.btcAsk - ev.btcBid)

    of tkBnTrade:
      bnTradeEvents += 1; bnTradeRate.increment(wallMs)
      totalRate.increment(wallMs)
      if ev.bnEventTimeMs > 0 and ev.bnTradeTimeMs > 0:
        bnTradeLatencies.add(float(ev.bnEventTimeMs - ev.bnTradeTimeMs))
      if ev.tradePrice > 0:
        if lastBnPrice > 0 and abs(ev.tradePrice - lastBnPrice) > 0.001:
          bnPriceSteps.add(abs(ev.tradePrice - lastBnPrice))
        lastBnPrice = ev.tradePrice

    of tkBnDepth:
      bnDepthEvents += 1; bnDepthRate.increment(wallMs)
      totalRate.increment(wallMs)

    of tkTopOfBook:
      pmEvents += 1
      totalRate.increment(wallMs)
      if ev.mid > 0:
        lastUpMid = ev.mid

    # ── Dashboard snapshot every 100ms ──
    if wallMs - lastDashPushMs >= 100:
      lastDashPushMs = wallMs

      # System metrics every ~1s
      if wallMs - lastSysMs >= 1000:
        sysMet.sample()
        lastSysMs = wallMs

      # Byte rates
      let pmBytesNow = ss.pmBytesTotal.load(moRelaxed)
      let bnBytesNow = ss.bnBytesTotal.load(moRelaxed)
      let dtMs = wallMs - lastByteMs
      var pmBytesPerSec, bnBytesPerSec: float32
      if dtMs > 0:
        pmBytesPerSec = float32((pmBytesNow - lastPmBytes).float * 1000.0 / dtMs.float)
        bnBytesPerSec = float32((bnBytesNow - lastBnBytes).float * 1000.0 / dtMs.float)
      lastPmBytes = pmBytesNow; lastBnBytes = bnBytesNow; lastByteMs = wallMs

      # Sparklines
      let pmQLen = cast[ptr SpscRing[FeedEvent]](ss.pmQ).len
      let refQLen = cast[ptr SpscRing[FeedEvent]](ss.refQ).len
      pmQSpark.push(int16(pmQLen), wallMs)
      refQSpark.push(int16(refQLen), wallMs)
      latSpark.push(int16(latHist.p50() div 1000), wallMs)  # us
      rateSpark.push(int16(totalRate.rate(wallMs)), wallMs)

      let elapsed = float(wallMs - int64(ss.windowStart) * 1000) / 1000.0

      var snap: DashboardSnapshot
      snap.epochMs = wallMs
      snap.elapsed = elapsed
      snap.phase = getPhase(elapsed, ss.duration)
      snap.instrumentCount = ss.registry.count
      snap.marketCount = ss.registry.marketCount
      snap.selectedMarket = ss.selectedMarket.load(moRelaxed)

      # Copy market groups
      for i in 0..<ss.registry.marketCount:
        snap.markets[i] = ss.registry.markets[i]

      # Fill instrument snapshots
      for i in 0..<ss.registry.count:
        snap.instruments[i].instrumentId = ss.registry.instruments[i].id
        snap.instruments[i].kind = ss.registry.instruments[i].kind
        snap.instruments[i].active = ss.registry.instruments[i].active
        if i < MaxInstruments:
          snap.instruments[i].bidPrice = instBidPrice[i]
          snap.instruments[i].askPrice = instAskPrice[i]
          snap.instruments[i].bidSize = instBidSize[i]
          snap.instruments[i].askSize = instAskSize[i]
          snap.instruments[i].spread = instSpread[i]
          snap.instruments[i].mid = instMid[i]
          snap.instruments[i].wmid = instWmid[i]
          snap.instruments[i].bidLevels = instBidLevels[i]
          snap.instruments[i].askLevels = instAskLevels[i]
          snap.instruments[i].totalBidDepth = instTotalBidDepth[i]
          snap.instruments[i].totalAskDepth = instTotalAskDepth[i]
          snap.instruments[i].bboChangesPerSec = instBboRate[i].rate(wallMs)
          snap.instruments[i].tradesPerSec = instTradeRate[i].rate(wallMs)
          snap.instruments[i].tradeCount = instTradeCount[i]
          snap.instruments[i].lastTradePrice = instLastTradePrice[i]
          snap.instruments[i].lastTradeSide = instLastTradeSide[i]
          snap.instruments[i].lastTradeSize = instLastTradeSize[i]
          # Imbalance from bid/ask sizes
          let bs = instBidSize[i]; let az = instAskSize[i]
          if bs + az > 0:
            snap.instruments[i].imbalance = float32((bs - az) / (bs + az))

      # Reference instrument (BN BBO) — stored using InstRef mapped to refIdx
      let refIdx = ss.registry.markets[0].refIdx.int
      if refIdx < MaxInstruments:
        snap.instruments[refIdx].bidPrice = lastBtcMid  # btcBid from telemetry
        # We track btc via the telem events; use last known values
        # The BN BBO data flows through telemetry events, we track lastBtcMid
        snap.instruments[refIdx].bidPrice = 0.0  # placeholder, BN reference is tracked differently
        snap.instruments[refIdx].askPrice = 0.0

      # Queue depths
      snap.pmQDepth = int32(pmQLen)
      snap.refQDepth = int32(refQLen)
      snap.telemQDepth = int32(telemQ.len)
      snap.pmQDrops = int64(cast[ptr SpscRing[FeedEvent]](ss.pmQ).drops)
      snap.refQDrops = int64(cast[ptr SpscRing[FeedEvent]](ss.refQ).drops)
      snap.telemQDrops = int64(telemQ.drops)

      # PM last message
      let pmLastNs = ss.pmLastMsgNs.load(moRelaxed)
      if pmLastNs > 0:
        snap.pmLastMsgMs = wallMs - int64((getMonoTime().ticks - pmLastNs).float / 1_000_000)
      else:
        snap.pmLastMsgMs = 0

      for i in 0..<MaxMarkets:
        let ns = ss.bnLastMsgNs[i].load(moRelaxed)
        if ns > 0:
          snap.bnLastMsgMs[i] = wallMs - int64((getMonoTime().ticks - ns).float / 1_000_000)

      # Network
      snap.pmRttUs = ss.pmRttUs.load(moRelaxed)
      snap.bnRttUs = ss.bnRttUs.load(moRelaxed)
      snap.pmBytesPerSec = pmBytesPerSec
      snap.bnBytesPerSec = bnBytesPerSec

      # Latency
      snap.latP50 = latHist.p50()
      snap.latP95 = latHist.p95()
      snap.latP99 = latHist.p99()
      snap.latP999 = latHist.p999()
      snap.latMin = latHist.minVal
      snap.latMax = latHist.maxVal
      snap.latSampleCount = int32(latHist.count)

      # Rates
      snap.totalEventsPerSec = totalRate.rate(wallMs)
      snap.pmEventsPerSec = pmRate.rate(wallMs)
      snap.bnBboPerSec = bnBboRate.rate(wallMs)
      snap.bnTradePerSec = bnTradeRate.rate(wallMs)
      snap.bnDepthPerSec = bnDepthRate.rate(wallMs)

      # Sparklines
      pmQSpark.copyTo(snap.pmQSparkline)
      refQSpark.copyTo(snap.refQSparkline)
      latSpark.copyTo(snap.latSparkline)
      rateSpark.copyTo(snap.rateSparkline)

      # System
      snap.cpuPercent = sysMet.cpuPercent
      snap.threadCount = sysMet.threadCount
      snap.rssBytes = sysMet.rssBytes
      snap.vmBytes = sysMet.vmBytes

      # Complementarity (up+down mid)
      for mi in 0..<ss.registry.marketCount:
        let mkt = ss.registry.markets[mi]
        let upMid = instMid[mkt.upIdx.int]
        let downMid = instMid[mkt.downIdx.int]
        if upMid > 0 and downMid > 0:
          snap.upPlusDown[mi] = upMid + downMid

      discard dashQ.tryPush(snap)

  # ── Fill summary ──
  ss.summary.tapeEvents = count
  ss.summary.pmEvents = pmEvents
  ss.summary.bnBboEvents = bnBboEvents
  ss.summary.bnTradeEvents = bnTradeEvents
  ss.summary.bnDepthEvents = bnDepthEvents
  ss.summary.pmTrades = pmTrades
  ss.summary.btcOpen = btcOpen
  ss.summary.btcClose = lastBtcMid
  ss.summary.finalUpProb = lastUpMid
  ss.summary.maxDrawdown = maxDD
  ss.summary.pmLargestTrade = largestTrade

  if bnTradeLatencies.len > 0:
    ss.summary.bnAvgTradeLatencyMs = bnTradeLatencies.foldl(a+b, 0.0) / bnTradeLatencies.len.float
    ss.summary.bnMaxTradeLatencyMs = bnTradeLatencies.max
  if bnSpreads.len > 0:
    ss.summary.bnAvgSpread = bnSpreads.foldl(a+b, 0.0) / bnSpreads.len.float
  if bnPriceSteps.len > 0:
    ss.summary.bnMinPriceStep = bnPriceSteps.min
  if interTradeTimes.len > 0:
    ss.summary.pmAvgInterTradeMs = interTradeTimes.foldl(a+b, 0.0) / interTradeTimes.len.float
    var sorted = interTradeTimes; sorted.sort()
    ss.summary.pmMedianInterTradeMs = sorted[sorted.len div 2]

  var maxBurst = 0.0
  for i in 0..<tradeTimestamps.len:
    var cnt = 0
    for j in i..<tradeTimestamps.len:
      if tradeTimestamps[j] - tradeTimestamps[i] <= 1.0: cnt += 1 else: break
    if cnt.float > maxBurst: maxBurst = cnt.float
  ss.summary.pmMaxBurstRate = maxBurst

# ═══════════════════════════════════════════════════════════════════════════
#  DASHBOARD THREAD — TUI rendering from DashboardSnapshot
# ═══════════════════════════════════════════════════════════════════════════

proc dashboardThread(ss: ptr SharedState) {.thread.} =
  let dashQ = cast[ptr SmallSpscRing[DashboardSnapshot]](ss.dashQ)
  enableRawMode()
  defer: disableRawMode()
  hideCursor()
  defer: showCursor()
  clearScreen()

  while ss.running.load(moRelaxed):
    # Drain to latest snapshot
    var snap: DashboardSnapshot
    var got = false
    while dashQ.tryPop(snap): got = true
    if not got:
      cpuRelax()
      sleep(10)
      continue

    cursorHome()
    renderDashboard(snap)
    flushStdout()

    let key = readKeyNonBlocking()
    case key
    of 'q', 'Q':
      ss.running.store(false, moRelaxed)
    of '1'..'9':
      let idx = int32(key.ord - '1'.ord)
      if idx < ss.registry.marketCount:
        ss.selectedMarket.store(idx, moRelaxed)
    else: discard

    sleep(16)  # ~60fps cap

# ═══════════════════════════════════════════════════════════════════════════
#  MARKET DISCOVERY
# ═══════════════════════════════════════════════════════════════════════════

proc findMarket(timeframe: string, windowStart: int): (string, string, string) =
  let tag = if timeframe == "5m": "5M" else: "15M"
  let url = &"{GammaApi}/events?limit=200&active=true&closed=false&tag_slug=up-or-down&tag_slug={tag}"
  let client = newHttpClient()
  defer: client.close()
  let resp = client.getContent(url)
  let events = parseJson(resp)
  for ev in events:
    for mkt in ev.getOrDefault("markets"):
      let slug = mkt.getOrDefault("slug").getStr("")
      if not slug.startsWith("btc-updown-"): continue
      let parts = slug.split('-')
      if parts.len < 4: continue
      let ts = try: parseInt(parts[^1]) except: continue
      if ts != windowStart: continue
      let tids = parseJson(mkt.getOrDefault("clobTokenIds").getStr("[]"))
      let outs = parseJson(mkt.getOrDefault("outcomes").getStr("[]"))
      if tids.len < 2: continue
      var upIdx = 0
      for i, o in outs.elems:
        if o.getStr("").toLowerAscii == "up": upIdx = i; break
      return (slug, tids[upIdx].getStr, tids[1 - upIdx].getStr)
  raise newException(ValueError, &"No market for window {windowStart}")

# ═══════════════════════════════════════════════════════════════════════════
#  REPORT
# ═══════════════════════════════════════════════════════════════════════════

proc fmtCommaMain(p: float): string =
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

proc printReport(ss: ptr SharedState) =
  let s = ss.summary
  let sep = "=".repeat(80)
  let dur = ss.duration
  let totalTime = float(dur + PreOpenSecs + PostCloseSecs)
  let btcMv = if s.btcOpen > 0: (s.btcClose - s.btcOpen) / s.btcOpen * 100 else: 0.0
  let outcome = if s.btcClose >= s.btcOpen: "UP" else: "DOWN"
  let slug = $ss.registry.markets[0].slug

  echo "\n" & sep
  echo "  WINDOW REPORT: " & slug
  echo "  Outcome: " & outcome & "  BTC: $" & fmtCommaMain(s.btcOpen) & " -> $" & fmtCommaMain(s.btcClose) & &" ({btcMv:+.4f}%)"
  echo &"  Tape: {$ss.tapeDir}/tape_{slug} ({s.tapeEvents} events)"
  let pmDrops = cast[ptr SpscRing[FeedEvent]](ss.pmQ).drops
  let refDrops = cast[ptr SpscRing[FeedEvent]](ss.refQ).drops
  let telemDrops = cast[ptr SpscRing[TelemetryEvent]](ss.telemQ).drops
  echo &"  Ring drops -- pm: {pmDrops}  ref: {refDrops}  telem: {telemDrops}"
  echo sep

  echo "\n## Event Counts"
  echo &"  PM book+delta:   {s.pmEvents:>8d}    ({s.pmEvents.float / totalTime:.0f}/sec)"
  echo &"  PM trades:       {s.pmTrades:>8d}    ({s.pmTrades.float / totalTime:.1f}/sec)"
  echo &"  BN bookTicker:   {s.bnBboEvents:>8d}    ({s.bnBboEvents.float / totalTime:.0f}/sec)"
  echo &"  BN trades:       {s.bnTradeEvents:>8d}    ({s.bnTradeEvents.float / totalTime:.0f}/sec)"
  echo &"  BN depth:        {s.bnDepthEvents:>8d}    ({s.bnDepthEvents.float / totalTime:.0f}/sec)"
  echo &"  Total tape:      {s.tapeEvents:>8d}"

  echo "\n## Binance Reference Quality"
  echo &"  Avg spread:              ${s.bnAvgSpread:.2f}"
  echo &"  Min price step:          ${s.bnMinPriceStep:.2f}"
  echo &"  Avg trade latency (E-T): {s.bnAvgTradeLatencyMs:.1f}ms"
  echo &"  Max trade latency:       {s.bnMaxTradeLatencyMs:.0f}ms"

  echo "\n## Polymarket Microstructure"
  echo &"  BBO changes:             {s.bboChanges} ({s.bboChanges.float / totalTime:.1f}/sec)"
  echo &"  Price reversals:         {s.priceReversals}"
  echo &"  Avg inter-trade:         {s.pmAvgInterTradeMs:.1f}ms"
  echo &"  Median inter-trade:      {s.pmMedianInterTradeMs:.1f}ms"
  echo &"  Peak burst rate:         {s.pmMaxBurstRate:.0f} trades/sec"
  echo &"  Largest trade:           {s.pmLargestTrade:.0f}"
  echo &"  Max drawdown:            {s.maxDrawdown:.3f}"
  echo &"  Final Up probability:    {s.finalUpProb:.3f}"

  echo "\n## Binance L2 Book Reconstruction"
  echo &"  Book snapshots applied:  {s.bnBookUpdates}"
  echo &"  BBO cross-validation:"
  echo &"    Matches:              {s.bnBboMatches}"
  echo &"    Mismatches:           {s.bnBboMismatches}"
  if s.bnBboMatches + s.bnBboMismatches > 0:
    let matchRate = s.bnBboMatches.float / (s.bnBboMatches + s.bnBboMismatches).float * 100
    echo &"    Match rate:            {matchRate:.1f}%"

  let baseName = $ss.tapeDir / "tape_" & slug
  let inputBinPath = baseName & ".input.bin"
  let stateBinPath = baseName & ".state.bin"
  var inputBinSize, stateBinSize: int64
  try: inputBinSize = getFileSize(inputBinPath) except: discard
  try: stateBinSize = getFileSize(stateBinPath) except: discard

  var inputZstSize, stateZstSize: int64
  if inputBinSize > 0:
    let rc1 = execShellCmd(&"zstd -q --rm -f {inputBinPath} -o {inputBinPath}.zst 2>/dev/null")
    if rc1 == 0:
      try: inputZstSize = getFileSize(inputBinPath & ".zst") except: discard
  if stateBinSize > 0:
    let rc2 = execShellCmd(&"zstd -q --rm -f {stateBinPath} -o {stateBinPath}.zst 2>/dev/null")
    if rc2 == 0:
      try: stateZstSize = getFileSize(stateBinPath & ".zst") except: discard

  echo "\n## Binary Tape"
  echo &"  Input tape:  {inputBinPath}"
  echo &"    Raw:       {inputBinSize div 1024} KB ({inputBinSize div 1024 div 1024} MB)"
  echo &"    Records:   {(inputBinSize - 64) div 128}"
  if inputZstSize > 0:
    echo &"    zstd:      {inputZstSize div 1024} KB ({inputBinSize.float / inputZstSize.float:.1f}x ratio)"
  echo &"  State tape:  {stateBinPath}"
  echo &"    Raw:       {stateBinSize div 1024} KB"
  echo &"    Records:   {(stateBinSize - 64) div 128}"
  if stateZstSize > 0:
    echo &"    zstd:      {stateZstSize div 1024} KB ({stateBinSize.float / stateZstSize.float:.1f}x ratio)"
  let totalBin = inputBinSize + stateBinSize
  let totalZst = inputZstSize + stateZstSize
  if totalBin > 0 and totalZst > 0:
    echo &"  Compression ratio:  {totalBin.float / totalZst.float:.1f}x"
  echo ""

# ═══════════════════════════════════════════════════════════════════════════
#  MAIN
# ═══════════════════════════════════════════════════════════════════════════

proc main() =
  var timeframe = "5m"
  var numWindows = 1
  var p = initOptParser(commandLineParams())
  while true:
    p.next()
    case p.kind
    of cmdEnd: break
    of cmdLongOption, cmdShortOption:
      case p.key
      of "timeframe", "t": timeframe = p.val
      of "windows", "w", "n": numWindows = parseInt(p.val)
      else: discard
    of cmdArgument: discard

  let dur = if timeframe == "5m": 300 else: 900
  let now = int(epochTime())
  let nextStart = ((now div dur) + 1) * dur
  let tapeDir = "data/tapes"
  createDir(tapeDir)

  let sep = "=".repeat(80)
  echo sep
  echo "  POLYMARKET CAPTURE -- MANTIS ARCHITECTURE (6 threads + TUI)"
  echo sep
  echo &"  Ring size:  {RingSize} slots x {sizeof(FeedEvent)}B = {RingSize * sizeof(FeedEvent) div 1024}KB per ring"
  echo &"  Timeframe:  {timeframe}   Windows: {numWindows}"

  for win in 0..<numWindows:
    let windowStart = nextStart + dur * win
    let captureEnd = windowStart + dur + PostCloseSecs

    echo &"\n  -- Window {win+1} --"
    echo &"  Discovering market for {fromUnix(windowStart.int64).utc.format(\"HH:mm:ss\")} UTC..."

    var slug, tokenUp, tokenDown: string
    try:
      (slug, tokenUp, tokenDown) = findMarket(timeframe, windowStart)
    except Exception as e:
      echo &"  ERROR: {e.msg}"; continue

    echo &"  Market: {slug}"
    echo &"  Up:     {tokenUp[0..15]}..."
    echo &"  Down:   {tokenDown[0..15]}..."

    # ── Allocate shared state ──
    var ss = cast[ptr SharedState](allocShared0(sizeof(SharedState)))
    ss.pmQ = cast[pointer](initSpscRing[FeedEvent](nil))
    ss.refQ = cast[pointer](initSpscRing[FeedEvent](nil))
    ss.telemQ = cast[pointer](initSpscRing[TelemetryEvent](nil))
    ss.dashQ = cast[pointer](initSmallSpscRing[DashboardSnapshot]())
    ss.monoBase = getMonoTime().ticks
    ss.windowStart = windowStart
    ss.captureEnd = captureEnd
    ss.duration = dur
    ss.tapeDir = toFixedStr(tapeDir)
    ss.running.store(true, moRelaxed)
    ss.selectedMarket.store(0, moRelaxed)

    # Set up registry — single market for now (Task 9 will generalize)
    ss.registry.count = 3  # Up, Down, Ref
    ss.registry.instruments[0] = InstrumentEntry(
      id: InstUp, kind: ikPmUpDown, symbol: toFixedLabel("UP"), active: true)
    ss.registry.instruments[1] = InstrumentEntry(
      id: InstDown, kind: ikPmUpDown, symbol: toFixedLabel("DOWN"), active: true)
    ss.registry.instruments[2] = InstrumentEntry(
      id: InstRef, kind: ikReference, symbol: toFixedLabel("BTC-REF"), active: true)
    ss.registry.marketCount = 1
    ss.registry.markets[0] = MarketGroup(
      label: toFixedLabel(slug.split('-')[0..2].join("-")),
      slug: toFixedStr(slug),
      upIdx: 0, downIdx: 1, refIdx: 2,
      timeframe: uint16(dur),
      windowStart: int64(windowStart),
      tokenUp: toFixedStr(tokenUp),
      tokenDown: toFixedStr(tokenDown),
    )

    # ── Wait for capture window ──
    let waitUntil = windowStart - PreOpenSecs
    let waitSecs = waitUntil - int(epochTime())
    if waitSecs > 0:
      echo &"  Waiting {waitSecs}s until capture..."
      sleep(waitSecs * 1000)

    echo "  Launching 6 threads..."

    # ── Spawn threads ──
    var tPm, tBn, tEngine, tTelem, tDash: Thread[ptr SharedState]
    createThread(tPm, pmIngestThread, ss)
    createThread(tBn, bnIngestThread, ss)
    createThread(tEngine, engineThread, ss)
    createThread(tTelem, telemetryThread, ss)
    createThread(tDash, dashboardThread, ss)

    # ── Main thread: simple sleep loop (dashboard handles display) ──
    while epochTime() < captureEnd.float and ss.running.load(moRelaxed):
      sleep(1000)

    # ── Shutdown ──
    ss.running.store(false, moRelaxed)
    let pmQ = cast[ptr SpscRing[FeedEvent]](ss.pmQ)
    let refQ = cast[ptr SpscRing[FeedEvent]](ss.refQ)
    discard pmQ.tryPush(FeedEvent(kind: ekShutdown))
    discard refQ.tryPush(FeedEvent(kind: ekShutdown))

    joinThread(tPm)
    joinThread(tBn)
    joinThread(tEngine)
    sleep(500)  # let telemetry drain
    joinThread(tTelem)
    joinThread(tDash)

    # ── Report ──
    printReport(ss)

    # ── Cleanup ──
    freeSpscRing(cast[ptr SpscRing[FeedEvent]](ss.pmQ))
    freeSpscRing(cast[ptr SpscRing[FeedEvent]](ss.refQ))
    freeSpscRing(cast[ptr SpscRing[TelemetryEvent]](ss.telemQ))
    freeSmallSpscRing(cast[ptr SmallSpscRing[DashboardSnapshot]](ss.dashQ))
    deallocShared(ss)

  quit(0)

main()
