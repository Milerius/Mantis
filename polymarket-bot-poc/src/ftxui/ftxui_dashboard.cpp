#include "ftxui_dashboard.h"
#include "snapshot_types.h"

#include <ftxui/dom/elements.hpp>
#include <ftxui/screen/screen.hpp>
#include <ftxui/dom/node.hpp>
#include <ftxui/screen/color.hpp>
#include <ftxui/dom/table.hpp>
#include <ftxui/dom/canvas.hpp>

#include <string>
#include <sstream>
#include <iomanip>
#include <cmath>
#include <cstring>
#include <iostream>
#include <algorithm>
#include <sys/select.h>
#include <unistd.h>
#include <termios.h>

using namespace ftxui;

// ── Helpers ─────────────────────────────────────────────────────────────────

static std::string fmtLabel(const FixedLabel& fl) {
    return std::string(fl.data, strnlen(fl.data, 32));
}

static std::string fmtStr(const FixedStr& fs) {
    return std::string(fs.data, strnlen(fs.data, 128));
}

static std::string fmtComma(double v) {
    std::ostringstream oss;
    oss << std::fixed << std::setprecision(2) << v;
    std::string s = oss.str();
    auto dot = s.find('.');
    std::string intPart = s.substr(0, dot);
    std::string decPart = s.substr(dot);
    std::string result;
    int n = static_cast<int>(intPart.size());
    for (int i = 0; i < n; i++) {
        if (i > 0 && (n - i) % 3 == 0) result += ',';
        result += intPart[i];
    }
    return result + decPart;
}

static std::string fmtLat(int64_t ns) {
    if (ns <= 0) return "---";
    if (ns < 1000) return std::to_string(ns) + "ns";
    if (ns < 1000000) {
        std::ostringstream o;
        o << std::fixed << std::setprecision(1) << (ns / 1000.0) << "us";
        return o.str();
    }
    std::ostringstream o;
    o << std::fixed << std::setprecision(1) << (ns / 1000000.0) << "ms";
    return o.str();
}

static std::string fmtBytes(int64_t b) {
    if (b < 1024) return std::to_string(b) + "B";
    if (b < 1024 * 1024) return std::to_string(b / 1024) + "KB";
    if (b < 1024LL * 1024 * 1024) return std::to_string(b / 1024 / 1024) + "MB";
    std::ostringstream o;
    o << std::fixed << std::setprecision(1) << (b / (1024.0 * 1024 * 1024)) << "GB";
    return o.str();
}

static std::string fmtRate(float r) {
    if (r < 1000) {
        std::ostringstream o;
        o << std::fixed << std::setprecision(0) << r << "/s";
        return o.str();
    }
    if (r < 1000000) {
        std::ostringstream o;
        o << std::fixed << std::setprecision(1) << (r / 1000) << "K/s";
        return o.str();
    }
    std::ostringstream o;
    o << std::fixed << std::setprecision(1) << (r / 1000000) << "M/s";
    return o.str();
}

static const char* phaseStr(Phase p) {
    switch (p) {
        case PreOpen:   return "PRE-OPEN";
        case Open:      return "OPEN";
        case Mid:       return "MID";
        case Late:      return "LATE";
        case Final:     return "FINAL";
        case PostClose: return "POST-CLOSE";
    }
    return "?";
}

// ── Dashboard State ─────────────────────────────────────────────────────────

struct FtxuiDashboard {
    std::string reset_position;
    struct termios orig_termios;
    bool raw_mode = false;

    void enableRawMode() {
        tcgetattr(0, &orig_termios);
        struct termios raw = orig_termios;
        raw.c_lflag &= ~(ECHO | ICANON);
        raw.c_cc[VMIN] = 0;
        raw.c_cc[VTIME] = 0;
        tcsetattr(0, TCSANOW, &raw);
        // Hide cursor
        std::cout << "\033[?25l";
        std::cout.flush();
        raw_mode = true;
    }

    void disableRawMode() {
        if (raw_mode) {
            tcsetattr(0, TCSANOW, &orig_termios);
            std::cout << "\033[?25h"; // Show cursor
            std::cout.flush();
            raw_mode = false;
        }
    }

    char pollKey() {
        fd_set fds;
        FD_ZERO(&fds);
        FD_SET(0, &fds);
        struct timeval tv = {0, 0};
        if (select(1, &fds, nullptr, nullptr, &tv) > 0) {
            char c;
            if (read(0, &c, 1) == 1) return c;
        }
        return 0;
    }

    // ── Panel builders ──────────────────────────────────────────────────

    Element buildHeader(const DashboardSnapshot* s) {
        Elements tabs;
        for (int i = 0; i < s->marketCount; i++) {
            auto label = fmtLabel(s->markets[i].label);
            auto t = text(" " + std::to_string(i + 1) + ":" + label + " ");
            if (i == s->selectedMarket)
                t = t | bold | inverted;
            else
                t = t | dim;
            tabs.push_back(t);
        }

        // Reference price
        std::string refPrice;
        if (s->selectedMarket < s->marketCount) {
            auto& mkt = s->markets[s->selectedMarket];
            auto& refInst = s->instruments[mkt.refIdx];
            if (refInst.mid > 0) {
                refPrice = fmtLabel(refInst.symbol) + ": $" + fmtComma(refInst.mid);
            }
        }

        return hbox({
            text(" MANTIS ") | bold | color(Color::Green),
            hbox(tabs),
            filler(),
            text(refPrice) | color(Color::Cyan),
            text("  "),
            text(std::string(phaseStr(s->phase)) + " ") | bold,
            text("+" + std::to_string(static_cast<int>(s->elapsed)) + "s") | dim,
            text(" "),
        });
    }

    Element buildUpBook(const DashboardSnapshot* s) {
        if (s->selectedMarket >= s->marketCount) return text("No market");
        auto& mkt = s->markets[s->selectedMarket];
        auto& inst = s->instruments[mkt.upIdx];

        std::ostringstream prob;
        prob << std::fixed << std::setprecision(2) << (inst.wmid * 100) << "%";

        // Depth ladder from upDepth
        std::vector<std::vector<std::string>> rows;
        rows.push_back({"DEPTH", "BID", "ASK", "DEPTH"});
        int levels = std::min(static_cast<int>(s->upDepth.bidCount),
                              static_cast<int>(s->upDepth.askCount));
        levels = std::min(levels, 8);
        for (int i = 0; i < levels; i++) {
            rows.push_back({
                std::to_string(static_cast<int>(s->upDepth.bids[i].size)),
                std::to_string(s->upDepth.bids[i].price).substr(0, 5),
                std::to_string(s->upDepth.asks[i].price).substr(0, 5),
                std::to_string(static_cast<int>(s->upDepth.asks[i].size)),
            });
        }
        if (rows.size() < 2) {
            // Fallback: just BBO
            std::ostringstream bp, ap;
            bp << std::fixed << std::setprecision(3) << inst.bidPrice;
            ap << std::fixed << std::setprecision(3) << inst.askPrice;
            rows.push_back({
                std::to_string(static_cast<int>(inst.bidSize)), bp.str(),
                ap.str(), std::to_string(static_cast<int>(inst.askSize))
            });
        }
        auto table = Table(rows);
        table.SelectAll().Border(LIGHT);
        table.SelectRow(0).Decorate(bold);
        table.SelectRow(0).SeparatorVertical(LIGHT);
        table.SelectColumn(0).DecorateCells(color(Color::Green));
        table.SelectColumn(1).DecorateCells(color(Color::Green));
        table.SelectColumn(2).DecorateCells(color(Color::Red));
        table.SelectColumn(3).DecorateCells(color(Color::Red));

        // Depth bar chart via Canvas
        auto depth_canvas = canvas([&](Canvas& c) {
            int w = c.width();
            int h = c.height();
            int cx = w / 2;
            double maxSize = 1;
            for (int i = 0; i < std::min(10, static_cast<int>(s->upDepth.bidCount)); i++)
                maxSize = std::max(maxSize, s->upDepth.bids[i].size);
            for (int i = 0; i < std::min(10, static_cast<int>(s->upDepth.askCount)); i++)
                maxSize = std::max(maxSize, s->upDepth.asks[i].size);

            int barH = std::max(1, h / 12);
            for (int i = 0; i < std::min(10, static_cast<int>(s->upDepth.bidCount)); i++) {
                int bw = static_cast<int>(s->upDepth.bids[i].size / maxSize * cx);
                int y = i * barH * 2;
                c.DrawBlockLine(cx - bw, y, cx, y, Color::Green);
            }
            for (int i = 0; i < std::min(10, static_cast<int>(s->upDepth.askCount)); i++) {
                int aw = static_cast<int>(s->upDepth.asks[i].size / maxSize * cx);
                int y = i * barH * 2;
                c.DrawBlockLine(cx, y, cx + aw, y, Color::Red);
            }
        });

        std::ostringstream stats;
        stats << "sp:" << std::fixed << std::setprecision(3) << inst.spread
              << " wmid:" << std::setprecision(4) << inst.wmid
              << " imb:" << std::setprecision(2) << std::showpos << inst.imbalance;

        return vbox({
            hbox({
                text("UP BOOK") | bold,
                filler(),
                text(prob.str()) | bold | color(Color::Cyan),
            }),
            table.Render(),
            depth_canvas | size(HEIGHT, EQUAL, 6),
            text(stats.str()) | dim,
        }) | flex;
    }

    Element buildDownBook(const DashboardSnapshot* s) {
        if (s->selectedMarket >= s->marketCount) return text("");
        auto& mkt = s->markets[s->selectedMarket];
        auto& inst = s->instruments[mkt.downIdx];
        std::ostringstream prob;
        prob << std::fixed << std::setprecision(2) << (inst.wmid * 100) << "%";
        std::ostringstream line;
        line << std::fixed << std::setprecision(3) << inst.bidPrice << " | " << inst.askPrice
             << "  sp:" << inst.spread;

        auto& upInst = s->instruments[mkt.upIdx];
        double upDown = (upInst.mid > 0 && inst.mid > 0) ? upInst.mid + inst.mid : 0;
        auto udColor = (upDown >= 0.998 && upDown <= 1.002) ? Color::Green :
                       (upDown >= 0.995 && upDown <= 1.005) ? Color::Yellow : Color::Red;
        std::ostringstream ud;
        ud << std::fixed << std::setprecision(4) << upDown;

        return vbox({
            hbox({
                text("DOWN BOOK") | bold,
                filler(),
                text(prob.str()) | bold | color(Color::YellowLight),
            }),
            text(line.str()),
            hbox({
                text("up+down: "),
                text(ud.str()) | color(udColor),
            }),
        });
    }

    Element buildReference(const DashboardSnapshot* s) {
        if (s->selectedMarket >= s->marketCount) return text("");
        auto& mkt = s->markets[s->selectedMarket];
        auto& refInst = s->instruments[mkt.refIdx];
        auto sym = fmtLabel(refInst.symbol);

        std::ostringstream sp;
        sp << std::fixed << std::setprecision(2) << refInst.spread;

        return vbox({
            hbox({
                text(sym) | bold,
                text(" (Binance)") | dim,
            }),
            text("$" + fmtComma(refInst.mid)) | bold | color(Color::Cyan),
            text("sp:$" + sp.str() + "  d20<>bbo:" +
                 std::to_string(static_cast<int>(refInst.bboMatchRate)) + "%") | dim,
        });
    }

    Element buildProbChart(const DashboardSnapshot* s) {
        if (s->selectedMarket >= s->marketCount) return text("No data");

        auto chart = canvas([&](Canvas& c) {
            int w = c.width();
            int h = c.height();
            // Grid lines at 25%, 50%, 75%
            for (float pct : {0.25f, 0.50f, 0.75f}) {
                int y = h - static_cast<int>(pct * h);
                for (int x = 0; x < w; x += 4)
                    c.DrawPoint(x, y, true, Color::GrayDark);
            }
            // Draw probability history
            int count = s->probHistoryCount;
            if (count < 2) return;
            int startIdx = (s->probHistoryIdx - count + PROB_HISTORY_LEN) % PROB_HISTORY_LEN;
            for (int i = 1; i < count && i < w; i++) {
                int i0 = (startIdx + i - 1) % PROB_HISTORY_LEN;
                int i1 = (startIdx + i) % PROB_HISTORY_LEN;
                float v0 = s->probHistory[i0];
                float v1 = s->probHistory[i1];
                int x0 = (i - 1) * w / count;
                int x1 = i * w / count;
                int y0 = h - static_cast<int>(v0 * h);
                int y1 = h - static_cast<int>(v1 * h);
                c.DrawPointLine(x0, y0, x1, y1, Color::Green);
            }
        });

        return vbox({
            text("PROBABILITY HISTORY") | bold | dim,
            chart | flex | size(HEIGHT, EQUAL, 15),
        });
    }

    Element buildLatencyHist(const DashboardSnapshot* s) {
        auto chart = canvas([&](Canvas& c) {
            int w = c.width();
            int h = c.height();
            // Approximate bars from percentiles (we lack raw histogram buckets)
            float barW = static_cast<float>(w) / 10;
            int64_t vals[] = {
                s->latP50, s->latP50, s->latP95, s->latP95,
                s->latP99, s->latP99, s->latP999, s->latP999,
                s->latMax, s->latMax
            };
            int64_t maxVal = std::max(s->latMax, static_cast<int64_t>(1));
            for (int i = 0; i < 10; i++) {
                int bh = static_cast<int>(static_cast<float>(vals[i]) / maxVal * h * 0.9f);
                int x0 = static_cast<int>(i * barW);
                int x1 = static_cast<int>((i + 1) * barW - 1);
                Color col = (i < 4) ? Color::Green : (i < 7) ? Color::Yellow : Color::Red;
                for (int y = h - bh; y < h; y++)
                    c.DrawBlockLine(x0, y, x1, y, col);
            }
        });

        return vbox({
            hbox({
                text("LATENCY") | bold | dim,
                text(" (n=" + std::to_string(s->latSampleCount) + ")") | dim,
            }),
            hbox({
                text("p50:") | dim, text(fmtLat(s->latP50)) | color(Color::Green), text(" "),
                text("p95:") | dim, text(fmtLat(s->latP95)) | color(Color::GreenLight), text(" "),
                text("p99:") | dim, text(fmtLat(s->latP99)) | color(Color::Yellow), text(" "),
                text("p999:") | dim, text(fmtLat(s->latP999)) | color(Color::RedLight),
            }),
            chart | flex | size(HEIGHT, EQUAL, 8),
            hbox({
                text("min:" + fmtLat(s->latMin) + " max:" + fmtLat(s->latMax)) | dim,
            }),
        });
    }

    Element buildRates(const DashboardSnapshot* s) {
        // Sparkline via graph()
        auto rate_graph = graph([&](int width, int height) -> std::vector<int> {
            std::vector<int> out(width, 0);
            int16_t maxElem = 1;
            for (int i = 0; i < SPARKLINE_LEN; i++)
                maxElem = std::max(maxElem, s->rateSparkline[i]);
            for (int i = 0; i < std::min(width, static_cast<int>(SPARKLINE_LEN)); i++) {
                out[i] = std::min(
                    static_cast<int>(s->rateSparkline[i] * height / maxElem),
                    height);
            }
            return out;
        });

        return vbox({
            text("RATES") | bold | dim,
            rate_graph | flex | size(HEIGHT, EQUAL, 4) | color(Color::BlueLight),
            hbox({
                text("pm:" + fmtRate(s->pmEventsPerSec)) | color(Color::Green), text(" "),
                text("bn:" + fmtRate(s->bnBboPerSec + s->bnTradePerSec + s->bnDepthPerSec)) | color(Color::Blue), text(" "),
                text("tot:" + fmtRate(s->totalEventsPerSec)) | bold,
            }),
        });
    }

    Element buildFeeds(const DashboardSnapshot* s) {
        int64_t pmStale = s->epochMs - s->pmLastMsgMs;
        auto pmDot = (pmStale < 100) ? color(Color::Green)
                   : (pmStale < 1000) ? color(Color::Yellow)
                   : color(Color::Red);
        int64_t bnStale = 0;
        for (int i = 0; i < s->marketCount; i++) {
            if (s->bnLastMsgMs[i] > 0) {
                int64_t st = s->epochMs - s->bnLastMsgMs[i];
                if (bnStale == 0 || st < bnStale) bnStale = st;
            }
        }
        auto bnDot = (bnStale < 100) ? color(Color::Green)
                   : (bnStale < 1000) ? color(Color::Yellow)
                   : color(Color::Red);

        return vbox({
            text("FEEDS") | bold | dim,
            hbox({text("● ") | pmDot, text("PM " + std::to_string(pmStale) + "ms")}),
            hbox({text("● ") | bnDot, text("BN " + std::to_string(bnStale) + "ms")}),
            text("PM:" + fmtBytes(static_cast<int64_t>(s->pmBytesPerSec)) +
                 "/s  BN:" + fmtBytes(static_cast<int64_t>(s->bnBytesPerSec)) + "/s") | dim,
        });
    }

    Element buildQueues(const DashboardSnapshot* s) {
        float pmPct = s->pmQDepth / 65536.0f;
        float refPct = s->refQDepth / 65536.0f;
        float telPct = s->telemQDepth / 65536.0f;
        auto qColor = [](float pct) -> Decorator {
            if (pct < 0.10f) return color(Color::Green);
            if (pct < 0.50f) return color(Color::Yellow);
            return color(Color::Red);
        };

        return vbox({
            text("QUEUES") | bold | dim,
            hbox({text("pm  "), gauge(pmPct) | qColor(pmPct) | flex,
                  text(" " + std::to_string(s->pmQDepth))}),
            hbox({text("ref "), gauge(refPct) | qColor(refPct) | flex,
                  text(" " + std::to_string(s->refQDepth))}),
            hbox({text("tel "), gauge(telPct) | qColor(telPct) | flex,
                  text(" " + std::to_string(s->telemQDepth))}),
            text("drops: " + std::to_string(s->pmQDrops) + "/" +
                 std::to_string(s->refQDrops) + "/" +
                 std::to_string(s->telemQDrops)) | dim,
        });
    }

    Element buildMicro(const DashboardSnapshot* s) {
        if (s->selectedMarket >= s->marketCount) return text("");
        auto& mkt = s->markets[s->selectedMarket];
        auto& inst = s->instruments[mkt.upIdx];
        std::string arrow = inst.moveDirection > 0 ? "▲"
                          : inst.moveDirection < 0 ? "▼"
                          : "-";
        std::string runStr;
        for (int i = 0; i < std::min(static_cast<int>(std::abs(inst.consecutiveMoves)), 5); i++)
            runStr += arrow;

        std::ostringstream lastTrade;
        lastTrade << std::fixed << std::setprecision(3) << inst.lastTradePrice;
        auto sideColor = inst.lastTradeSide == 0 ? Color::Green : Color::Red;
        auto sideStr = inst.lastTradeSide == 0 ? "B" : "S";

        return vbox({
            text("MICROSTRUCTURE") | bold | dim,
            hbox({
                text("BBO/s:"),
                text(std::to_string(static_cast<int>(inst.bboChangesPerSec))) | color(Color::Green),
                text(" rev:"),
                text(std::to_string(inst.priceReversals)) | color(Color::Yellow),
                text(" burst:"),
                text(std::to_string(static_cast<int>(inst.burstRate))),
            }),
            hbox({
                text("run:" + runStr + "(" + std::to_string(inst.consecutiveMoves) + ")"),
                text(" last:" + lastTrade.str() + " "),
                text(sideStr) | color(sideColor),
                text(" " + std::to_string(static_cast<int>(inst.lastTradeSize))),
            }),
        });
    }

    Element buildTradeTape(const DashboardSnapshot* s) {
        std::vector<std::vector<std::string>> rows;
        rows.push_back({"TIME", "SIDE", "PRICE", "SIZE"});
        for (int i = 0; i < MAX_TRADES; i++) {
            int idx = (s->tradeWriteIdx - 1 - i + MAX_TRADES) % MAX_TRADES;
            auto& t = s->trades[idx];
            if (t.epochMs == 0) continue;
            time_t secs = t.epochMs / 1000;
            struct tm tm;
            gmtime_r(&secs, &tm);
            char timeBuf[16];
            snprintf(timeBuf, sizeof(timeBuf), "%02d:%02d:%02d",
                     tm.tm_hour, tm.tm_min, tm.tm_sec);
            std::ostringstream price;
            price << std::fixed << std::setprecision(3) << t.price;
            rows.push_back({
                timeBuf,
                t.side == 0 ? "BUY" : "SELL",
                price.str(),
                "$" + std::to_string(static_cast<int>(t.size)),
            });
        }
        auto table = Table(rows);
        table.SelectAll().Border(LIGHT);
        table.SelectRow(0).Decorate(bold | dim);
        // Color BUY green, SELL red
        for (size_t r = 1; r < rows.size(); r++) {
            auto col = (rows[r][1] == "BUY") ? Color::Green : Color::Red;
            table.SelectCell(1, static_cast<int>(r)).Decorate(color(col));
        }
        return vbox({
            text("TRADE TAPE") | bold | dim,
            table.Render() | flex | yframe,
        });
    }

    Element buildStatusBar(const DashboardSnapshot* s) {
        return hbox({
            text(" [1-9]market [q]quit [l]latency [t]tape [d]debug") | dim,
            filler(),
            text("THR:" + std::to_string(s->threadCount) +
                 " CPU:" + std::to_string(static_cast<int>(s->cpuPercent)) + "%" +
                 " RSS:" + fmtBytes(s->rssBytes)) | dim,
            text(" "),
        });
    }

    // ── Main layout ─────────────────────────────────────────────────────

    Element buildLayout(const DashboardSnapshot* s) {
        auto left_col = vbox({
            buildUpBook(s) | flex,
            separator(),
            buildDownBook(s),
            separator(),
            buildReference(s),
        }) | size(WIDTH, EQUAL, 45) | border;

        auto center_col = vbox({
            buildProbChart(s) | flex,
            separator(),
            buildLatencyHist(s),
            separator(),
            buildRates(s),
        }) | flex | border;

        auto right_col = vbox({
            buildFeeds(s),
            separator(),
            buildQueues(s),
            separator(),
            buildMicro(s),
            separator(),
            buildTradeTape(s) | flex,
        }) | size(WIDTH, EQUAL, 38) | border;

        return vbox({
            buildHeader(s),
            separator(),
            hbox({left_col, center_col, right_col}) | flex,
            buildStatusBar(s),
        });
    }
};

// ── C API ───────────────────────────────────────────────────────────────────

extern "C" {

FtxuiDashboard* dashboard_create() {
    auto* d = new FtxuiDashboard();
    d->enableRawMode();
    // Clear screen
    std::cout << "\033[2J\033[H";
    std::cout.flush();
    return d;
}

void dashboard_destroy(FtxuiDashboard* d) {
    if (d) {
        d->disableRawMode();
        delete d;
    }
}

char dashboard_render(FtxuiDashboard* d, const void* snapshot_ptr) {
    auto* snap = static_cast<const DashboardSnapshot*>(snapshot_ptr);

    auto document = d->buildLayout(snap);
    auto screen = Screen::Create(Dimension::Full(), Dimension::Fit(document));
    Render(screen, document);
    std::cout << d->reset_position;
    screen.Print();
    d->reset_position = screen.ResetPosition();

    return d->pollKey();
}

} // extern "C"
