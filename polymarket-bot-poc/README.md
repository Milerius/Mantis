# Polymarket Bot POC

Real-time multi-market observation tool for Polymarket prediction markets with FTXUI terminal dashboard.

Built with Nim, using the Mantis architecture: lock-free SPSC rings, zero-alloc hot path, multi-threaded pipeline.

## Architecture

```
PM Ingest (1 thread)     BN Ingest (1 thread, 9 WS)
  Polymarket WS            Binance bookTicker/trade/depth20
  JSON parse               per symbol: BTC, SOL, ETH
  FeedEvent -> pm_q        FeedEvent -> ref_q
        |                        |
        +------- Engine (1 thread, busy-spin) -------+
                 books[N] array, BBO tracking
                 reversal detection, queue runs
                 TelemetryEvent -> telem_q
                            |
               Telemetry (1 thread, side-path)
                 mmap binary tape writer
                 rolling stats, latency histogram
                 DashboardSnapshot -> dash_q (10Hz)
                            |
               Dashboard (1 thread)
                 FTXUI 3-column trading terminal
                 Canvas charts, depth bars, trade tape
```

## Features

- **Multi-market**: BTC, SOL, ETH up-or-down 5m/15m markets discovered automatically
- **6-thread pipeline**: ingest(2) + engine + telemetry + dashboard + main
- **FTXUI dashboard**: professional trading terminal with:
  - Depth ladder (UP/DOWN books, 8 levels)
  - Binance L2 book (depth20 reconstruction, 5 levels)
  - Probability history line chart (braille canvas, 60s window)
  - Engine latency histogram (p50/p95/p99/p999)
  - Event rate bar sparkline
  - Depth bar charts (bid/ask horizontal bars)
  - Queue gauge bars with color thresholds
  - Feed status with staleness indicators
  - Trade tape (last 8 trades, BUY/SELL colored)
  - Market tab switching (1-9 keys)
- **Binary tape**: mmap-backed input/state tapes with zstd compression
- **Low latency**: p50 ~42ns, p99 ~1us engine processing
- **Low CPU**: ~9% with yield-on-idle (was 200% with pure busy-spin)

## Prerequisites

- **Nim** >= 2.2 with nimble packages: `ws`, `constantine`
- **CMake** >= 3.14 (for FTXUI build)
- **C++ compiler**: clang 15+ or gcc 12+ (C++17)
- **OpenSSL 3**: `brew install openssl@3` on macOS

### Install Nim dependencies

```bash
nimble install ws
# constantine should already be available from the Mantis project
```

## Build

### FTXUI mode (recommended)

```bash
cd polymarket-bot-poc
make        # builds FTXUI libs + Nim binary
```

This does:
1. `cmake` fetches FTXUI v5.0.0 and builds static libs
2. `nim cpp -d:ftxui` compiles the Nim binary with C++ backend

### Fallback mode (no FTXUI, ANSI escape codes)

```bash
cd polymarket-bot-poc
make nim-fallback    # nim c (C backend, no FTXUI)
```

## Run

```bash
cd polymarket-bot-poc
./src/main --timeframe=5m --windows=1
```

Options:
- `--timeframe=5m` or `--timeframe=15m`
- `--windows=N` — number of consecutive capture windows

### Controls

| Key | Action |
|-----|--------|
| `1`-`9` | Switch market tab |
| `q` | Quit |

## Logs

Errors and lifecycle events are written to `mantis.log`:

```bash
tail -f mantis.log
```

## Output

Binary tapes are written to `data/tapes/`:
- `tape_<slug>.input.bin` — every event (128B records, mmap)
- `tape_<slug>.state.bin` — BBO changes only
- Both compressed with zstd after capture

## Project Structure

```
src/
  main.nim              Entry point, 6-thread orchestration
  types.nim             All shared types (single source of truth)
  spsc.nim              Lock-free SPSC ring buffers (65K + 256 slots)
  engine_book.nim       PM integer book + BN depth20 book
  tape_format.nim       mmap binary tape writer/reader
  stats.nim             Rolling counters, latency histogram, sparklines
  system_metrics.nim    CPU/RSS/VM via getrusage + mach_task_info
  mach_helper.c         macOS memory stats (extern "C")
  ftxui_bindings.nim    {.importcpp.} bindings for FTXUI
  dashboard_ftxui.nim   Pure Nim FTXUI dashboard (3-column layout)
  dashboard.nim         ANSI fallback dashboard
  ftxui/
    CMakeLists.txt      Fetches + builds FTXUI static libs
Makefile                Top-level build orchestration
nim.cfg                 Compiler config
```
