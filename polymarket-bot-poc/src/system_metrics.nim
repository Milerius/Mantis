# polymarket-bot-poc/src/system_metrics.nim — CPU, memory, thread metrics
#
# macOS: mach_task_info for memory, getrusage for CPU
# Linux: /proc/self/stat, /proc/self/status

import std/posix
import std/strutils
from std/times import epochTime

# macOS mach APIs used via {.emit.} in sampleMemory to avoid C/C++ type conflicts

type
  SystemMetrics* = object
    cpuPercent*: float32
    rssBytes*: int64
    vmBytes*: int64
    threadCount*: int32
    # Internal state for CPU delta
    lastUserUs: int64
    lastSysUs: int64
    lastWallMs: int64

proc init*(sm: var SystemMetrics; threadCount: int32) =
  sm.threadCount = threadCount
  sm.lastWallMs = int64(epochTime() * 1000)
  var usage: RUsage
  if getrusage(RUSAGE_SELF, addr usage) == 0:
    sm.lastUserUs = int64(usage.ru_utime.tv_sec) * 1_000_000'i64 +
                    int64(usage.ru_utime.tv_usec)
    sm.lastSysUs  = int64(usage.ru_stime.tv_sec) * 1_000_000'i64 +
                    int64(usage.ru_stime.tv_usec)

when defined(macosx):
  {.compile: "mach_helper.c".}
  proc c_sample_memory(rss: ptr int64, vm: ptr int64) {.importc: "sample_memory_mach", cdecl.}

proc sampleMemory(sm: var SystemMetrics) =
  when defined(macosx):
    c_sample_memory(addr sm.rssBytes, addr sm.vmBytes)
  elif defined(linux):
    try:
      let status = readFile("/proc/self/status")
      for line in status.splitLines:
        if line.startsWith("VmRSS:"):
          sm.rssBytes = parseInt(line.split()[1]) * 1024
        elif line.startsWith("VmSize:"):
          sm.vmBytes = parseInt(line.split()[1]) * 1024
    except: discard

proc sample*(sm: var SystemMetrics) =
  let nowMs = int64(epochTime() * 1000)

  # CPU via getrusage delta
  var usage: RUsage
  if getrusage(RUSAGE_SELF, addr usage) == 0:
    let userUs = int64(usage.ru_utime.tv_sec) * 1_000_000'i64 +
                 int64(usage.ru_utime.tv_usec)
    let sysUs  = int64(usage.ru_stime.tv_sec) * 1_000_000'i64 +
                 int64(usage.ru_stime.tv_usec)
    let cpuUs  = (userUs - sm.lastUserUs) + (sysUs - sm.lastSysUs)
    let wallUs = (nowMs - sm.lastWallMs) * 1000
    if wallUs > 0:
      sm.cpuPercent = float32(cpuUs.float / wallUs.float * 100.0)
    sm.lastUserUs = userUs
    sm.lastSysUs  = sysUs
  sm.lastWallMs = nowMs

  sampleMemory(sm)
