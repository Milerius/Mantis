#!/bin/bash
BOT_DIR="/Users/milerius/Documents/Mantis/polymarket"
LOG="$BOT_DIR/multi_bot.log"
STATUS_LOG="$BOT_DIR/bot_status.log"

echo "========================================" >> "$STATUS_LOG"
echo "$(date '+%Y-%m-%d %H:%M:%S') — Multi-Bot Status" >> "$STATUS_LOG"

PID=$(pgrep -f "multi_strategy_bot.py" | head -1)
if [ -z "$PID" ]; then
    echo "  STATUS: DEAD" >> "$STATUS_LOG"
else
    echo "  STATUS: ALIVE (PID $PID)" >> "$STATUS_LOG"
fi

tail -5 "$LOG" 2>/dev/null | sed 's/^/    /' >> "$STATUS_LOG"
REPLAYS=$(ls -1 "$BOT_DIR/window_replay_multi/"*.json 2>/dev/null | wc -l | tr -d ' ')
echo "  REPLAYS: $REPLAYS" >> "$STATUS_LOG"
echo "" >> "$STATUS_LOG"
