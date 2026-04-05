#pragma once
#include <cstdint>

// Mirror of Nim types — must match types.nim field order exactly

static constexpr int MAX_INSTRUMENTS = 16;
static constexpr int MAX_MARKETS = 8;
static constexpr int MAX_TRADES = 8;
static constexpr int SPARKLINE_LEN = 60;
static constexpr int PROB_HISTORY_LEN = 120;
static constexpr int DEPTH_LEVELS = 20;

struct FixedStr { char data[128]; };
struct FixedLabel { char data[32]; };

enum Phase : int32_t {
    PreOpen = 0, Open = 1, Mid = 2, Late = 3, Final = 4, PostClose = 5
};

enum InstrumentKind : int32_t {
    ikPmUpDown = 0, ikReference = 1
};

struct InstrumentSnapshot {
    uint32_t instrumentId;
    InstrumentKind kind;
    bool active;
    FixedLabel symbol;
    double bidPrice, askPrice;
    double bidSize, askSize;
    double spread, mid, wmid;
    float imbalance;
    int32_t bidLevels, askLevels;
    double totalBidDepth, totalAskDepth;
    int32_t bboChanges;
    float bboChangesPerSec;
    int32_t priceReversals;
    int16_t consecutiveMoves;
    int8_t moveDirection;
    int32_t tradeCount;
    float tradesPerSec;
    float burstRate;
    double lastTradePrice;
    uint8_t lastTradeSide;
    double lastTradeSize;
    float bboMatchRate;
    float avgTradeLatencyMs;
};

struct MarketGroup {
    FixedLabel label;
    FixedStr slug;
    int8_t upIdx, downIdx, refIdx;
    uint16_t timeframe;
    int64_t windowStart;
    FixedStr tokenUp, tokenDown;
};

struct TradeTick {
    int64_t epochMs;
    uint32_t instrumentId;
    double price;
    double size;
    uint8_t side;
};

struct DepthLevel {
    double price;
    double size;
};

struct DepthLadder {
    DepthLevel bids[DEPTH_LEVELS];
    DepthLevel asks[DEPTH_LEVELS];
    int32_t bidCount, askCount;
};

struct DashboardSnapshot {
    int64_t epochMs;
    double elapsed;
    Phase phase;
    int32_t instrumentCount;
    InstrumentSnapshot instruments[MAX_INSTRUMENTS];
    int32_t marketCount;
    MarketGroup markets[MAX_MARKETS];
    int32_t selectedMarket;
    // Queue health
    int32_t pmQDepth, refQDepth, telemQDepth;
    int64_t pmQDrops, refQDrops, telemQDrops;
    int32_t pmQHighWater, refQHighWater, telemQHighWater;
    int16_t pmQSparkline[SPARKLINE_LEN];
    int16_t refQSparkline[SPARKLINE_LEN];
    // Feed health
    int64_t pmLastMsgMs;
    int64_t bnLastMsgMs[MAX_MARKETS];
    int32_t pmSeqGaps, bnSeqGaps;
    uint8_t wsStatePm, wsStateBn;
    // Network
    int32_t pmRttUs, bnRttUs;
    int64_t pmLastPingMs, bnLastPingMs;
    float pmBytesPerSec, bnBytesPerSec;
    // Latency
    int64_t latP50, latP95, latP99, latP999;
    int64_t latMin, latMax;
    int32_t latSampleCount;
    int16_t latSparkline[SPARKLINE_LEN];
    // Rates
    float totalEventsPerSec, pmEventsPerSec;
    float bnBboPerSec, bnTradePerSec, bnDepthPerSec;
    int16_t rateSparkline[SPARKLINE_LEN];
    // System
    float cpuPercent;
    int32_t threadCount;
    int64_t rssBytes, vmBytes;
    // Complementarity
    double upPlusDown[MAX_MARKETS];
    // Trade tape
    TradeTick trades[MAX_TRADES];
    int32_t tradeWriteIdx;
    // Reserved
    uint8_t reserved[128];
    // New fields (added after reserved to avoid breaking existing layout)
    DepthLadder upDepth, downDepth;
    float probHistory[PROB_HISTORY_LEN];
    int32_t probHistoryIdx;
    int32_t probHistoryCount;
};
