# test_dashboard.nim — Quick smoke test for FTXUI dashboard rendering
# Tests that the dashboard handles empty/zero snapshot data without crashing.

import types
import dashboard_ftxui
import std/[os]

proc main() =
  echo "Testing FTXUI dashboard with empty snapshot..."

  initFtxuiDashboard()
  defer: destroyFtxuiDashboard()

  # Test 1: Completely empty snapshot (all zeros)
  var snap: DashboardSnapshot
  echo "  Test 1: empty snapshot..."
  for i in 0..<5:
    let key = renderDashboardFtxui(snap)
    if key == 'q': break
    sleep(100)

  # Test 2: Snapshot with markets but no data
  echo "  Test 2: markets but no data..."
  snap.marketCount = 3
  snap.instrumentCount = 9
  snap.selectedMarket = 0
  snap.markets[0].label = toFixedLabel("BTC-5m")
  snap.markets[0].upIdx = 0
  snap.markets[0].downIdx = 1
  snap.markets[0].refIdx = 2
  snap.markets[1].label = toFixedLabel("ETH-5m")
  snap.markets[1].upIdx = 3
  snap.markets[1].downIdx = 4
  snap.markets[1].refIdx = 5
  snap.markets[2].label = toFixedLabel("SOL-5m")
  snap.markets[2].upIdx = 6
  snap.markets[2].downIdx = 7
  snap.markets[2].refIdx = 8
  snap.instruments[0].symbol = toFixedLabel("BTC_UP")
  snap.instruments[1].symbol = toFixedLabel("BTC_DN")
  snap.instruments[2].symbol = toFixedLabel("BTCUSDT")
  snap.epochMs = 1775398800000'i64
  snap.elapsed = -14.0
  snap.phase = PreOpen
  for i in 0..<5:
    let key = renderDashboardFtxui(snap)
    if key == 'q': break
    sleep(100)

  # Test 3: Snapshot with some book data
  echo "  Test 3: with book data..."
  snap.instruments[0].bidPrice = 0.650
  snap.instruments[0].askPrice = 0.660
  snap.instruments[0].bidSize = 28
  snap.instruments[0].askSize = 93
  snap.instruments[0].spread = 0.010
  snap.instruments[0].mid = 0.655
  snap.instruments[0].wmid = 0.6542
  snap.instruments[0].imbalance = -0.64
  snap.instruments[0].bidLevels = 65
  snap.instruments[0].askLevels = 34
  snap.instruments[0].active = true
  snap.instruments[2].mid = 83421.50
  snap.instruments[2].bidPrice = 83421.25
  snap.instruments[2].askPrice = 83421.75
  snap.instruments[2].symbol = toFixedLabel("BTCUSDT")
  snap.latP50 = 125
  snap.latP95 = 334
  snap.latP99 = 583
  snap.latP999 = 2000
  snap.latMin = 41
  snap.latMax = 118200
  snap.latSampleCount = 1000
  snap.totalEventsPerSec = 1700
  snap.pmEventsPerSec = 651
  snap.phase = Mid
  snap.elapsed = 73.0

  for i in 0..<20:
    let key = renderDashboardFtxui(snap)
    if key == 'q': break
    sleep(200)

  echo "\n  All tests passed — no crash!"

main()
