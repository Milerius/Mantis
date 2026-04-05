# tape_format.nim — Binary tape format for Mantis market data
#
# Two files per capture session:
#   Input tape:  every event the engine receives (raw HotEvent equivalent)
#   State tape:  engine output on each BBO price change (TopOfBook + derived)
#
# Both are fixed-size records, repr(C), zero serialization — just memcpy.
# Written via mmap for zero-copy, zero-alloc hot path writes.

import std/[os, posix]

const
  TapeMagic* = [byte 0x4D, 0x41, 0x4E, 0x54, 0x49, 0x53, 0x01, 0x00]  # "MANTIS\x01\x00"
  TapeVersion* = 1'u32
  InputRecordSize* = 128  # padded to power of 2 for alignment
  StateRecordSize* = 128

type
  TapeHeader* {.packed.} = object
    magic*: array[8, byte]
    version*: uint32
    recordSize*: uint32
    startTs*: uint64        # epoch nanoseconds
    instrumentCount*: uint32
    flags*: uint32
    reserved*: array[32, byte]
  # 64 bytes total

  # Input tape record — raw event data
  InputRecord* {.packed.} = object
    # Event identity
    kind*: uint8            #  1B — event kind
    instrumentId*: uint8    #  1B — 0=Up, 1=Down, 2=Ref
    side*: uint8            #  1B — 0=BUY, 1=SELL
    flags*: uint8           #  1B — LAST_IN_BATCH etc
    seqNo*: uint32          #  4B — per-source sequence
    # Timing
    wallNs*: int64          #  8B — monotonic ns from capture start
    epochMs*: int64         #  8B — wall clock ms
    # Price/Size
    price*: float64         #  8B
    size*: float64          #  8B
    priceMilli*: int16      #  2B
    pad0*: array[6, byte]   #  6B — alignment
    # BN-specific
    bnEventTimeMs*: int64   #  8B
    bnTradeTimeMs*: int64   #  8B
    bnBid*: float64         #  8B
    bnAsk*: float64         #  8B
    bnBidQty*: float64      #  8B
    bnAskQty*: float64      #  8B
    bnUpdateId*: int64      #  8B
    # Global
    globalSeq*: uint64      #  8B — monotonic across all sources
    pad1*: array[16, byte]  # 16B — pad to 128 bytes
    # ───── 128 bytes total

  # State tape record — engine output on BBO change
  StateRecord* {.packed.} = object
    # TopOfBook
    bidPrice*: float64      #  8B
    askPrice*: float64      #  8B
    bidSize*: float64       #  8B
    askSize*: float64       #  8B
    microPrice*: float64    #  8B
    spread*: float64        #  8B
    # Instrument + timing
    instrumentId*: uint8    #  1B
    phase*: uint8           #  1B
    pad0*: array[2, byte]   #  2B
    seqNo*: uint32          #  4B
    wallNs*: int64          #  8B
    epochMs*: int64         #  8B
    # Derived metrics
    imbalance5*: float32    #  4B
    imbalance10*: float32   #  4B
    bidDepth5*: float64     #  8B
    askDepth5*: float64     #  8B
    bidLevels*: uint16      #  2B
    askLevels*: uint16      #  2B
    # Reference
    btcMid*: float64        #  8B
    btcImbalance5*: float32 #  4B
    pad1*: array[8, byte]   #  8B — pad to 128 bytes
    globalSeq*: uint64      #  8B
    # ───── 128 bytes total

# ── mmap-backed tape writer ────────────────────────────────────────────

const
  DefaultTapeSize = 256 * 1024 * 1024  # 256MB initial — enough for a 5min window

type
  MmapTapeWriter* = object
    fd: cint
    data: pointer
    capacity: int
    offset: int
    recordSize: int
    recordCount: int
    path: string

proc initMmapTapeWriter*(path: string, header: TapeHeader, capacity: int = DefaultTapeSize): MmapTapeWriter =
  result.path = path
  result.recordSize = header.recordSize.int
  result.capacity = capacity

  # Create file with capacity
  result.fd = posix.open(path.cstring, O_RDWR or O_CREAT or O_TRUNC, 0o644)
  if result.fd < 0:
    raise newException(IOError, "Cannot open tape file: " & path)

  # Extend to capacity
  if ftruncate(result.fd, Off(capacity)) != 0:
    discard posix.close(result.fd)
    raise newException(IOError, "Cannot resize tape file")

  # mmap
  result.data = mmap(nil, capacity, PROT_READ or PROT_WRITE, MAP_SHARED, result.fd, 0)
  if result.data == MAP_FAILED:
    discard posix.close(result.fd)
    raise newException(IOError, "Cannot mmap tape file")

  # Write header
  copyMem(result.data, unsafeAddr header, sizeof(TapeHeader))
  result.offset = sizeof(TapeHeader)

proc appendInput*(tw: var MmapTapeWriter, rec: InputRecord) {.inline.} =
  ## Append one input record. Zero-copy — just memcpy to mmap.
  if tw.offset + InputRecordSize > tw.capacity:
    return  # tape full — drop (could extend in production)
  copyMem(cast[pointer](cast[uint](tw.data) + tw.offset.uint), unsafeAddr rec, InputRecordSize)
  tw.offset += InputRecordSize
  tw.recordCount += 1

proc appendState*(tw: var MmapTapeWriter, rec: StateRecord) {.inline.} =
  ## Append one state record.
  if tw.offset + StateRecordSize > tw.capacity:
    return
  copyMem(cast[pointer](cast[uint](tw.data) + tw.offset.uint), unsafeAddr rec, StateRecordSize)
  tw.offset += StateRecordSize
  tw.recordCount += 1

proc finalize*(tw: var MmapTapeWriter) =
  ## Truncate file to actual size and close.
  if tw.data != nil and tw.data != MAP_FAILED:
    discard munmap(tw.data, tw.capacity)
    tw.data = nil
  if tw.fd >= 0:
    discard ftruncate(tw.fd, Off(tw.offset))
    discard posix.close(tw.fd)
    tw.fd = -1

# ── mmap-backed tape reader ────────────────────────────────────────────

type
  MmapTapeReader* = object
    data: pointer
    size: int
    offset: int
    recordSize: int
    header*: TapeHeader
    fd: cint

proc initMmapTapeReader*(path: string): MmapTapeReader =
  var stat: Stat
  if posix.stat(path.cstring, stat) != 0:
    raise newException(IOError, "Cannot stat tape file: " & path)

  result.size = stat.st_size.int
  result.fd = posix.open(path.cstring, O_RDONLY, 0)
  if result.fd < 0:
    raise newException(IOError, "Cannot open tape file: " & path)

  result.data = mmap(nil, result.size, PROT_READ, MAP_PRIVATE, result.fd, 0)
  if result.data == MAP_FAILED:
    discard posix.close(result.fd)
    raise newException(IOError, "Cannot mmap tape file")

  # Read header
  if result.size < sizeof(TapeHeader):
    raise newException(IOError, "Tape file too small for header")
  copyMem(addr result.header, result.data, sizeof(TapeHeader))

  if result.header.magic != TapeMagic:
    raise newException(IOError, "Invalid tape magic")

  result.recordSize = result.header.recordSize.int
  result.offset = sizeof(TapeHeader)

  # Prefetch the entire file — madvise may not be available on all platforms
  when defined(linux) or defined(macosx):
    proc c_madvise(addr1: pointer, len: csize_t, advice: cint): cint {.importc: "madvise", header: "<sys/mman.h>".}
    const MADV_SEQ = 2  # MADV_SEQUENTIAL
    discard c_madvise(result.data, result.size.csize_t, MADV_SEQ)

proc recordCount*(tr: MmapTapeReader): int =
  (tr.size - sizeof(TapeHeader)) div tr.recordSize

proc readInput*(tr: var MmapTapeReader, rec: var InputRecord): bool =
  if tr.offset + InputRecordSize > tr.size: return false
  copyMem(addr rec, cast[pointer](cast[uint](tr.data) + tr.offset.uint), InputRecordSize)
  tr.offset += InputRecordSize
  true

proc readState*(tr: var MmapTapeReader, rec: var StateRecord): bool =
  if tr.offset + StateRecordSize > tr.size: return false
  copyMem(addr rec, cast[pointer](cast[uint](tr.data) + tr.offset.uint), StateRecordSize)
  tr.offset += StateRecordSize
  true

proc close*(tr: var MmapTapeReader) =
  if tr.data != nil and tr.data != MAP_FAILED:
    discard munmap(tr.data, tr.size)
    tr.data = nil
  if tr.fd >= 0:
    discard posix.close(tr.fd)
    tr.fd = -1

# Compile-time size checks
static:
  assert sizeof(TapeHeader) == 64, "TapeHeader must be 64 bytes"
  assert sizeof(InputRecord) == 128, "InputRecord must be 128 bytes"
  assert sizeof(StateRecord) == 128, "StateRecord must be 128 bytes"
