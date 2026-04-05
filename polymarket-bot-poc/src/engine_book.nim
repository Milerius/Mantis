# polymarket-bot-poc/src/engine_book.nim — Order book data structures for engine thread
#
# Integer-indexed (PriceMilli 0-1000) for PM, level-array for BN depth20.

import constantine/platforms/intrinsics/compiler_optim_hints  # prefetch

const BnDepthLevels* = 20

type
  PriceMilli* = int16

  EngineBookSide* = object
    levels*: array[1001, float]
    bestPrice*: PriceMilli
    dirty*: bool

  EngineBook* = object
    bids*, asks*: EngineBookSide
    changeCount*: int

  BnBookLevel* = object
    price*: float64
    qty*: float64

  BnBook* = object
    bids*: array[BnDepthLevels, BnBookLevel]
    asks*: array[BnDepthLevels, BnBookLevel]
    bidCount*: int
    askCount*: int
    lastUpdateId*: int64
    valid*: bool

proc clearSide*(bs: var EngineBookSide) =
  for i in 0..1000: bs.levels[i] = 0
  bs.bestPrice = 0; bs.dirty = true

proc applyLevel*(bs: var EngineBookSide, pm: PriceMilli, size: float, isBid: bool) =
  if pm < 0 or pm > 1000: return
  if pm > 0: prefetch(bs.levels[pm - 1].addr, Read, HighTemporalLocality)
  if pm < 1000: prefetch(bs.levels[pm + 1].addr, Read, HighTemporalLocality)
  bs.levels[pm] = size
  bs.dirty = true

proc recalcBest*(bs: var EngineBookSide, isBid: bool) =
  if isBid:
    bs.bestPrice = 0
    for i in countdown(1000'i16, 0'i16):
      if bs.levels[i] > 0: bs.bestPrice = i; break
  else:
    bs.bestPrice = 0
    for i in 0'i16..1000'i16:
      if bs.levels[i] > 0: bs.bestPrice = i; break
  bs.dirty = false

proc bestPriceF*(bs: var EngineBookSide, isBid: bool): (float, float) =
  if bs.dirty: bs.recalcBest(isBid)
  if bs.bestPrice == 0: (0.0, 0.0)
  else: (bs.bestPrice.float / 1000.0, bs.levels[bs.bestPrice])

proc mid*(b: var EngineBook): float =
  let (bp, _) = b.bids.bestPriceF(true)
  let (ap, _) = b.asks.bestPriceF(false)
  if bp > 0 and ap > 0: (bp + ap) / 2.0 else: 0.0

proc spread*(b: var EngineBook): float =
  let (bp, _) = b.bids.bestPriceF(true)
  let (ap, _) = b.asks.bestPriceF(false)
  if bp > 0 and ap > 0: ap - bp else: 0.0

proc weightedMid*(b: var EngineBook): float =
  let (bp, bs) = b.bids.bestPriceF(true)
  let (ap, az) = b.asks.bestPriceF(false)
  if bp > 0 and ap > 0 and (bs + az) > 0: (bp * az + ap * bs) / (bs + az)
  else: 0.0

proc bidLevelCount*(b: EngineBook): int =
  for i in 0..1000:
    if b.bids.levels[i] > 0: result += 1

proc askLevelCount*(b: EngineBook): int =
  for i in 0..1000:
    if b.asks.levels[i] > 0: result += 1

proc totalBidDepth*(b: EngineBook): float64 =
  for i in 0..1000: result += b.bids.levels[i]

proc totalAskDepth*(b: EngineBook): float64 =
  for i in 0..1000: result += b.asks.levels[i]

proc imbalance*(b: var EngineBook): float32 =
  let (_, bs) = b.bids.bestPriceF(true)
  let (_, az) = b.asks.bestPriceF(false)
  if bs + az > 0: float32((bs - az) / (bs + az)) else: 0'f32

proc bnBestBid*(b: BnBook): (float64, float64) =
  if b.bidCount > 0: (b.bids[0].price, b.bids[0].qty) else: (0.0, 0.0)

proc bnBestAsk*(b: BnBook): (float64, float64) =
  if b.askCount > 0: (b.asks[0].price, b.asks[0].qty) else: (0.0, 0.0)

proc bnMid*(b: BnBook): float64 =
  let (bp, _) = b.bnBestBid()
  let (ap, _) = b.bnBestAsk()
  if bp > 0 and ap > 0: (bp + ap) / 2.0 else: 0.0

proc bnSpread*(b: BnBook): float64 =
  let (bp, _) = b.bnBestBid()
  let (ap, _) = b.bnBestAsk()
  if bp > 0 and ap > 0: ap - bp else: 0.0

proc bnImbalance*(b: BnBook, levels: int): float64 =
  var bidDepth, askDepth: float64
  for i in 0..<min(levels, b.bidCount): bidDepth += b.bids[i].qty
  for i in 0..<min(levels, b.askCount): askDepth += b.asks[i].qty
  if bidDepth + askDepth > 0: (bidDepth - askDepth) / (bidDepth + askDepth) else: 0.0
