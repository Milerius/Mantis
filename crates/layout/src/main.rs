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
}
