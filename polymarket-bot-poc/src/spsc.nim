# polymarket-bot-poc/src/spsc.nim — Generic lock-free SPSC ring buffer
#
# Cache-line padded, acquire/release ordering.
# From constantine/weave patterns.

import std/atomics
import types
import constantine/threadpool/crossthread/backoff  # Eventcount
import constantine/platforms/intrinsics/compiler_optim_hints  # prefetch

type
  SpscRing*[T] = object
    head* {.align(CacheLineBytes).}: Atomic[int]
    drops*: int
    tail* {.align(CacheLineBytes).}: Atomic[int]
    notify*: ptr Eventcount
    buf*: array[RingSize, T]

proc initSpscRing*[T](ec: ptr Eventcount = nil): ptr SpscRing[T] =
  result = cast[ptr SpscRing[T]](allocShared0(sizeof(SpscRing[T])))
  result.head.store(0, moRelaxed)
  result.tail.store(0, moRelaxed)
  result.notify = ec

proc freeSpscRing*[T](ring: ptr SpscRing[T]) =
  if ring != nil: deallocShared(ring)

proc tryPush*[T](ring: ptr SpscRing[T], item: T): bool {.inline.} =
  let h = ring.head.load(moRelaxed)
  let next = (h + 1) and RingMask
  if next == ring.tail.load(moAcquire):
    ring.drops += 1
    return false
  ring.buf[h] = item
  ring.head.store(next, moRelease)
  if ring.notify != nil:
    ring.notify[].wake()
  true

proc tryPop*[T](ring: ptr SpscRing[T], item: var T): bool {.inline.} =
  let t = ring.tail.load(moRelaxed)
  if t == ring.head.load(moAcquire):
    return false
  let next = (t + 1) and RingMask
  prefetch(ring.buf[next].addr, Read, HighTemporalLocality)
  item = ring.buf[t]
  ring.tail.store(next, moRelease)
  true

proc len*[T](ring: ptr SpscRing[T]): int =
  let h = ring.head.load(moRelaxed)
  let t = ring.tail.load(moRelaxed)
  (h - t + RingSize) and RingMask

# ── Small ring variant for dashboard (256 slots) ──

type
  SmallSpscRing*[T] = object
    head* {.align(CacheLineBytes).}: Atomic[int]
    drops*: int
    tail* {.align(CacheLineBytes).}: Atomic[int]
    buf*: array[DashRingSize, T]

proc initSmallSpscRing*[T](): ptr SmallSpscRing[T] =
  result = cast[ptr SmallSpscRing[T]](allocShared0(sizeof(SmallSpscRing[T])))
  result.head.store(0, moRelaxed)
  result.tail.store(0, moRelaxed)

proc freeSmallSpscRing*[T](ring: ptr SmallSpscRing[T]) =
  if ring != nil: deallocShared(ring)

proc tryPush*[T](ring: ptr SmallSpscRing[T], item: T): bool {.inline.} =
  let h = ring.head.load(moRelaxed)
  let next = (h + 1) and DashRingMask
  if next == ring.tail.load(moAcquire):
    ring.drops += 1
    return false
  ring.buf[h] = item
  ring.head.store(next, moRelease)
  true

proc tryPop*[T](ring: ptr SmallSpscRing[T], item: var T): bool {.inline.} =
  let t = ring.tail.load(moRelaxed)
  if t == ring.head.load(moAcquire):
    return false
  let next = (t + 1) and DashRingMask
  item = ring.buf[t]
  ring.tail.store(next, moRelease)
  true

proc len*[T](ring: ptr SmallSpscRing[T]): int =
  let h = ring.head.load(moRelaxed)
  let t = ring.tail.load(moRelaxed)
  (h - t + DashRingSize) and DashRingMask
