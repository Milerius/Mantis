# mantis-queue

Lock-free SPSC (single-producer, single-consumer) ring buffer for the Mantis SDK.

`no_std` by default. Optimized for sub-3ns push+pop latency on modern hardware.

## Architecture

```
┌─────────────────────────────────────────────────────┐
│                   Preset Aliases                     │
│  SpscRing<T,N>  SpscRingHeap<T>  SpscRingInstrumented│
└───────────────────────┬─────────────────────────────┘
                        │
          ┌─────────────▼──────────────┐
          │     RawRing<T,S,I,P,Instr> │  ← public handle
          │     try_push / try_pop     │
          └─────────────┬──────────────┘
                        │
          ┌─────────────▼──────────────┐
          │     RingEngine (internal)   │
          │  head ─── CachePadded      │  Acquire/Release
          │  tail ─── CachePadded      │  + cached remote
          │  storage ─ InlineStorage   │    indices
          │           or HeapStorage   │
          └─────────────┬──────────────┘
                        │
          ┌─────────────▼──────────────┐
          │    raw::slot (unsafe)       │
          │  write / read / drop_slot  │
          └────────────────────────────┘
```

### Strategy Pattern

The engine is parameterized by:

- **`Storage<T>`** — where slots live (`InlineStorage` stack, `HeapStorage` heap)
- **`IndexStrategy`** — how indices wrap (`Pow2Masked`)
- **`PushPolicy`** — full behavior (`ImmediatePush`)
- **`Instrumentation`** — hooks (`NoInstr`, `CountingInstr`)

### Memory Layout

- Head and tail on separate 128-byte cache lines (`CachePadded`) to prevent false sharing
- Works on both Intel (64B lines) and Apple Silicon (128B lines)
- Power-of-2 capacity for branchless index wrapping via bitwise AND

## Preset Types

| Type | Storage | Instrumentation | Use case |
|---|---|---|---|
| `SpscRing<T, N>` | Inline (stack) | None | Default, fastest |
| `SpscRingHeap<T>` | Heap (alloc) | None | Runtime-sized capacity |
| `SpscRingInstrumented<T, N>` | Inline | `CountingInstr` | Debug/profiling |

## Usage

```rust
use mantis_queue::SpscRing;

let mut ring = SpscRing::<u64, 1024>::new();

// Push
ring.try_push(42).unwrap();

// Pop
let val = ring.try_pop().unwrap();
assert_eq!(val, 42);
```

### Split Producer/Consumer

```rust
use mantis_queue::spsc_ring;

let (mut producer, mut consumer) = spsc_ring::<u64, 1024>();
producer.try_push(1).unwrap();
let val = consumer.try_pop().unwrap();
```

### Heap-allocated (runtime capacity)

```rust
use mantis_queue::SpscRingHeap;

let mut ring = SpscRingHeap::<u64>::with_capacity(1024);
ring.try_push(42).unwrap();
```

## Features

| Feature | Default | Effect |
|---|---|---|
| `alloc` | off | Enables `HeapStorage`, `SpscRingHeap`, split handles |
| `std` | off | Enables `alloc` + std library |

## Safety

All unsafe code is isolated in `crates/queue/src/raw/`. The crate root denies unsafe. Every unsafe block has a `// SAFETY:` comment documenting invariants. Validated by Miri on every PR.

## Performance

Measured on Apple M4 Pro (aarch64), `cargo bench`:

| Workload | ns/op |
|---|---|
| single push+pop (u64) | 2.14 |
| burst 100 (u64) | 422 |
| burst 1000 (u64) | 4105 |
| single push+pop ([u8; 64]) | 26.8 |
| single push+pop ([u8; 256]) | 94.6 |
