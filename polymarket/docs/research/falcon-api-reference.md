# Falcon API Quick Reference

**Base URL:** `https://narrative.agent.heisenberg.so`
**Endpoint:** `POST /api/v2/semantic/retrieve/parameterized`

## Agent IDs

| Endpoint | agent_id | Key Required Param |
|---|---|---|
| Polymarket Markets | 574 | — (all optional) |
| Polymarket Trades | 556 | wallet_proxy, condition_id, market_slug, side, start_time, end_time |
| Polymarket Candlesticks | 568 | token_id (required), interval, start_time, end_time |
| Polymarket Orderbook | 572 | token_id (required), start_time, end_time |
| Polymarket PnL | 569 | wallet (required), granularity, start_time (YYYY-MM-DD), end_time |
| Polymarket Leaderboard | 579 | wallet_address, leaderboard_period |
| H-Score Leaderboard | 584 | min_win_rate_15d, max_win_rate_15d, min_roi_15d, etc. |
| Wallet 360 | 581 | proxy_wallet (required), window_days (1,3,7,15) |
| Market Insights | 575 | min_volume_24h, min_liquidity_percentile, volume_trend |
| Kalshi Markets | 565 | ticker, event_ticker, title, status |
| Kalshi Trades | 573 | ticker, start_time, end_time |
| Social Pulse | 585 | keywords (curly braces), hours_back |

## Notes
- All params are strings (except pagination limit/offset which are integers)
- Pagination max limit: 200
- PnL uses YYYY-MM-DD dates, everything else uses Unix timestamps (seconds)
- Social Pulse keywords must be in curly braces: "{Trump,election,MAGA}"
