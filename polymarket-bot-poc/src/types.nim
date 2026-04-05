# polymarket-bot-poc/src/types.nim — Shared types for capture pipeline
#
# All cross-thread, fixed-size, no-GC types live here.
# Imported by: polymarket_capture, dashboard, stats, engine_book

import std/[atomics, monotimes]
import constantine/threadpool/crossthread/backoff  # Eventcount

const
  CacheLineBytes* = 128
  MaxInstruments* = 16
  MaxMarkets* = 8
  MaxTrades* = 8
  SparklineLen* = 60       # 60 samples at 1/sec = 1 minute history
  RingSize* = 1 shl 16
  RingMask* = RingSize - 1
  DashRingSize* = 256
  DashRingMask* = DashRingSize - 1
  PingIntervalSec* = 10.0
  StatusIntervalMs* = 3_000
  PreOpenSecs* = 15
  PostCloseSecs* = 15

  SideBuy*: uint8 = 0
  SideSell*: uint8 = 1
  FlagLastInBatch*: uint8 = 1

type
  FixedStr* = array[128, char]
  FixedLabel* = array[32, char]

  Phase* = enum
    PreOpen = 0
    Open = 1
    Mid = 2
    Late = 3
    Final = 4
    PostClose = 5

  EventKind* = enum
    ekNone = 0
    ekPmBookClear = 1
    ekPmDelta = 2
    ekPmTrade = 3
    ekBnBbo = 4
    ekBnTrade = 5
    ekBnDepth = 6
    ekShutdown = 7

  FeedFlags* = uint8

  FeedEvent* = object
    kind*: EventKind
    instrumentId*: uint32
    localNs*: int64
    localEpochMs*: int64
    price*: float64
    size*: float64
    side*: uint8
    flags*: FeedFlags
    priceMilli*: int16
    seqNo*: uint32
    bnEventTimeMs*: int64
    bnTradeTimeMs*: int64
    bnBid*: float64
    bnAsk*: float64
    bnBidQty*: float64
    bnAskQty*: float64
    bnUpdateId*: int64
    bnDepthBidLevels*: int16
    bnDepthAskLevels*: int16
    bnIsBuyerMaker*: bool
    pad1*: array[3, byte]

  TelemetryKind* = enum
    tkBookUpdate = 0
    tkTrade = 1
    tkBnBbo = 2
    tkBnTrade = 3
    tkBnDepth = 4
    tkTopOfBook = 5

  TelemetryEvent* = object
    kind*: TelemetryKind
    phase*: Phase
    localNs*: int64
    localEpochMs*: int64
    elapsed*: float64
    bidPrice*: float64
    askPrice*: float64
    bidSize*: float64
    askSize*: float64
    mid*: float64
    spread*: float64
    weightedMid*: float64
    bidLevels*: int32
    askLevels*: int32
    totalBidDepth*: float64
    totalAskDepth*: float64
    btcMid*: float64
    btcBid*: float64
    btcAsk*: float64
    evKind*: EventKind
    instId*: uint32
    tradePrice*: float64
    tradeSize*: float64
    tradeSide*: uint8
    flags*: FeedFlags
    seqNo*: uint32
    bnEventTimeMs*: int64
    bnTradeTimeMs*: int64
    bnBidQty*: float64
    bnAskQty*: float64
    engineLatencyNs*: int64

  InstrumentKind* = enum
    ikPmUpDown = 0
    ikReference = 1

  InstrumentEntry* = object
    id*: uint32
    kind*: InstrumentKind
    symbol*: FixedLabel
    active*: bool

  MarketGroup* = object
    label*: FixedLabel
    slug*: FixedStr
    upIdx*, downIdx*, refIdx*: int8
    timeframe*: uint16
    windowStart*: int64
    tokenUp*, tokenDown*: FixedStr

  InstrumentRegistry* = object
    count*: int32
    instruments*: array[MaxInstruments, InstrumentEntry]
    marketCount*: int32
    markets*: array[MaxMarkets, MarketGroup]

  InstrumentSnapshot* = object
    instrumentId*: uint32
    kind*: InstrumentKind
    active*: bool
    symbol*: FixedLabel          # "BTC_UP", "SOLUSDT", etc.
    bidPrice*, askPrice*: float64
    bidSize*, askSize*: float64
    spread*, mid*, wmid*: float64
    imbalance*: float32
    bidLevels*, askLevels*: int32
    totalBidDepth*, totalAskDepth*: float64
    bboChanges*: int32
    bboChangesPerSec*: float32
    priceReversals*: int32
    consecutiveMoves*: int16
    moveDirection*: int8
    tradeCount*: int32
    tradesPerSec*: float32
    burstRate*: float32
    lastTradePrice*: float64
    lastTradeSide*: uint8
    lastTradeSize*: float64
    bboMatchRate*: float32
    avgTradeLatencyMs*: float32

  TradeTick* = object
    epochMs*: int64
    instrumentId*: uint32
    price*: float64
    size*: float64
    side*: uint8

  DepthLevel* = object
    price*: float64
    size*: float64

  DepthLadder* = object
    bids*: array[20, DepthLevel]
    asks*: array[20, DepthLevel]
    bidCount*, askCount*: int32

  DashboardSnapshot* = object
    epochMs*: int64
    elapsed*: float64
    phase*: Phase
    instrumentCount*: int32
    instruments*: array[MaxInstruments, InstrumentSnapshot]
    marketCount*: int32
    markets*: array[MaxMarkets, MarketGroup]
    selectedMarket*: int32
    pmQDepth*, refQDepth*, telemQDepth*: int32
    pmQDrops*, refQDrops*, telemQDrops*: int64
    pmQHighWater*, refQHighWater*, telemQHighWater*: int32
    pmQSparkline*: array[SparklineLen, int16]
    refQSparkline*: array[SparklineLen, int16]
    pmLastMsgMs*: int64
    bnLastMsgMs*: array[MaxMarkets, int64]
    pmSeqGaps*, bnSeqGaps*: int32
    wsStatePm*, wsStateBn*: uint8
    pmRttUs*: int32
    bnRttUs*: int32
    pmLastPingMs*: int64
    bnLastPingMs*: int64
    pmBytesPerSec*: float32
    bnBytesPerSec*: float32
    latP50*, latP95*, latP99*, latP999*: int64
    latMin*, latMax*: int64
    latSampleCount*: int32
    latSparkline*: array[SparklineLen, int16]
    totalEventsPerSec*: float32
    pmEventsPerSec*: float32
    bnBboPerSec*, bnTradePerSec*, bnDepthPerSec*: float32
    rateSparkline*: array[SparklineLen, int16]
    cpuPercent*: float32
    threadCount*: int32
    rssBytes*: int64
    vmBytes*: int64
    upPlusDown*: array[MaxMarkets, float64]
    trades*: array[MaxTrades, TradeTick]
    tradeWriteIdx*: int32
    reserved*: array[128, byte]
    # FTXUI depth ladder data
    upDepth*: DepthLadder
    downDepth*: DepthLadder
    # Probability history
    probHistory*: array[120, float32]
    probHistoryIdx*: int32
    probHistoryCount*: int32

  CaptureSummary* = object
    tapeEvents*: int
    pmEvents*, bnBboEvents*, bnTradeEvents*, bnDepthEvents*: int
    pmTrades*: int
    bboChanges*, priceReversals*: int
    btcOpen*, btcClose*: float64
    finalUpProb*: float64
    maxDrawdown*: float64
    pmLargestTrade*: float64
    pmMaxBurstRate*: float64
    bnAvgTradeLatencyMs*, bnMaxTradeLatencyMs*: float64
    bnAvgSpread*, bnMinPriceStep*: float64
    pmAvgInterTradeMs*, pmMedianInterTradeMs*: float64
    bnBookUpdates*: int
    bnBboMatches*, bnBboMismatches*: int

  SharedState* = object
    # Rings (typed as pointer — actual ring types resolved at use site)
    pmQ*: pointer
    refQ*: pointer
    telemQ*: pointer
    dashQ*: pointer
    # Engine parking
    engineEc*: Eventcount
    # Timing
    monoBase*: int64
    windowStart*: int
    captureEnd*: int
    duration*: int
    # Instrument registry
    registry*: InstrumentRegistry
    # Lifecycle
    running*: Atomic[bool]
    # Network metrics (written by ingest, read by telemetry)
    pmRttUs*: Atomic[int32]
    bnRttUs*: Atomic[int32]
    pmBytesTotal*: Atomic[int64]
    bnBytesTotal*: Atomic[int64]
    pmLastMsgNs*: Atomic[int64]
    bnLastMsgNs*: array[MaxMarkets, Atomic[int64]]
    # Dashboard control
    selectedMarket*: Atomic[int32]
    # Output
    summary*: CaptureSummary
    tapeDir*: FixedStr

proc toFixedStr*(s: string): FixedStr =
  for i in 0..<min(s.len, 127): result[i] = s[i]

proc toFixedLabel*(s: string): FixedLabel =
  for i in 0..<min(s.len, 31): result[i] = s[i]

proc `$`*(fs: FixedStr): string =
  result = ""
  for c in fs:
    if c == '\0': break
    result.add(c)

proc `$`*(fl: FixedLabel): string =
  result = ""
  for c in fl:
    if c == '\0': break
    result.add(c)

proc getPhase*(elapsed: float, duration: int): Phase {.inline.} =
  if elapsed < 0: PreOpen
  elif elapsed / duration.float <= 0.10: Open
  elif elapsed / duration.float <= 0.70: Mid
  elif elapsed / duration.float <= 0.90: Late
  elif elapsed / duration.float <= 1.0: Final
  else: PostClose

proc phaseStr*(p: Phase): string =
  case p
  of PreOpen: "PRE-OPEN"
  of Open: "OPEN"
  of Mid: "MID"
  of Late: "LATE"
  of Final: "FINAL"
  of PostClose: "POST-CLOSE"

proc nowNs*(base: int64): int64 {.inline.} =
  getMonoTime().ticks - base
