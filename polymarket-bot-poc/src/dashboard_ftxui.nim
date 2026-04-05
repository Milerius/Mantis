# polymarket-bot-poc/src/dashboard_ftxui.nim — FTXUI FFI bindings
#
# Wraps the C API from ftxui_dashboard.h.
# Used when compiled with -d:ftxui.

import types

# Link against the static libs (built by cmake in src/ftxui/build/)
{.passL: "src/ftxui/build/libftxui_dashboard.a".}
{.passL: "src/ftxui/build/_deps/ftxui-build/libftxui-dom.a".}
{.passL: "src/ftxui/build/_deps/ftxui-build/libftxui-screen.a".}
{.passL: "src/ftxui/build/_deps/ftxui-build/libftxui-component.a".}
{.passL: "-lc++".}
{.passC: "-Isrc/ftxui".}

type
  FtxuiDashboardPtr = pointer

proc dashboard_create(): FtxuiDashboardPtr
  {.importc, header: "ftxui_dashboard.h".}
proc dashboard_destroy(d: FtxuiDashboardPtr)
  {.importc, header: "ftxui_dashboard.h".}
proc dashboard_render(d: FtxuiDashboardPtr, snap: pointer): char
  {.importc, header: "ftxui_dashboard.h".}

# ── Public API ──

var gDashboard: FtxuiDashboardPtr = nil

proc initFtxuiDashboard*() =
  gDashboard = dashboard_create()

proc destroyFtxuiDashboard*() =
  if gDashboard != nil:
    dashboard_destroy(gDashboard)
    gDashboard = nil

proc renderDashboardFtxui*(snap: var DashboardSnapshot): char =
  if gDashboard == nil: return '\0'
  dashboard_render(gDashboard, addr snap)
