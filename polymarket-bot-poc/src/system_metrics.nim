# polymarket-bot-poc/src/system_metrics.nim — CPU, memory, thread metrics
#
# macOS: mach_task_info for memory, getrusage for CPU
# Linux: /proc/self/stat, /proc/self/status

import std/posix
from std/times import epochTime

when defined(macosx):
  type
    MachTaskBasicInfo {.importc: "struct mach_task_basic_info",
                        completeStruct,
                        header: "<mach/task_info.h>".} = object
      virtual_size: uint64
      resident_size: uint64
      resident_size_max: uint64
      user_time: Timeval
      system_time: Timeval
      policy: cint
      suspend_count: cint

  const
    # mach_task_basic_info_count from <mach/task_info.h>: always 10 on macOS
    MACH_TASK_BASIC_INFO_FLAVOR: cuint = 20
    MACH_TASK_BASIC_INFO_COUNT: cuint  = 10

  proc mach_task_self(): cuint {.importc, header: "<mach/mach.h>".}
  proc task_info(target_task: cuint; flavor: cuint; task_info_out: pointer;
                 task_info_outCnt: ptr cuint): cint {.importc, header: "<mach/task_info.h>".}

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

proc sampleMemory(sm: var SystemMetrics) =
  when defined(macosx):
    var info: MachTaskBasicInfo
    var count: cuint = MACH_TASK_BASIC_INFO_COUNT
    if task_info(mach_task_self(), MACH_TASK_BASIC_INFO_FLAVOR,
                 addr info, addr count) == 0:
      sm.rssBytes = int64(info.resident_size)
      sm.vmBytes  = int64(info.virtual_size)
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
