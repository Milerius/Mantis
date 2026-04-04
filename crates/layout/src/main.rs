//! CLI entry point for struct layout inspection.

use std::io::Write;

fn main() {
    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    writeln!(out, "=== Mantis Layout Report ===\n").ok();
    writeln!(
        out,
        "{}",
        mantis_layout::inspect::<mantis_types::QueueError>("QueueError")
    )
    .ok();
    writeln!(
        out,
        "{}",
        mantis_layout::inspect::<mantis_types::SeqNum>("SeqNum")
    )
    .ok();
    writeln!(
        out,
        "{}",
        mantis_layout::inspect::<mantis_types::SlotIndex>("SlotIndex")
    )
    .ok();

    writeln!(out, "=== Event Layout Report ===\n").ok();
    writeln!(
        out,
        "{}",
        mantis_layout::inspect::<mantis_events::HotEvent>("HotEvent")
    )
    .ok();
    writeln!(
        out,
        "{}",
        mantis_layout::inspect::<mantis_events::EventHeader>("EventHeader")
    )
    .ok();
    writeln!(
        out,
        "{}",
        mantis_layout::inspect::<mantis_events::EventBody>("EventBody")
    )
    .ok();
    writeln!(
        out,
        "{}",
        mantis_layout::inspect::<mantis_events::BookDeltaPayload>("BookDeltaPayload")
    )
    .ok();
    writeln!(
        out,
        "{}",
        mantis_layout::inspect::<mantis_events::TradePayload>("TradePayload")
    )
    .ok();
    writeln!(
        out,
        "{}",
        mantis_layout::inspect::<mantis_events::TopOfBookPayload>("TopOfBookPayload")
    )
    .ok();
    writeln!(
        out,
        "{}",
        mantis_layout::inspect::<mantis_events::FillPayload>("FillPayload")
    )
    .ok();
    writeln!(
        out,
        "{}",
        mantis_layout::inspect::<mantis_events::OrderAckPayload>("OrderAckPayload")
    )
    .ok();
    writeln!(
        out,
        "{}",
        mantis_layout::inspect::<mantis_events::OrderRejectPayload>("OrderRejectPayload")
    )
    .ok();
    writeln!(
        out,
        "{}",
        mantis_layout::inspect::<mantis_events::TimerPayload>("TimerPayload")
    )
    .ok();
    writeln!(
        out,
        "{}",
        mantis_layout::inspect::<mantis_events::HeartbeatPayload>("HeartbeatPayload")
    )
    .ok();
}
