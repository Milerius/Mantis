#!/usr/bin/env python3
"""Compare SPSC benchmark results from Rust and C++ JSON files.

Reads *_run_*.json files from a directory, groups by implementation,
and prints a side-by-side markdown comparison table sorted by median p50.
"""

import json
import os
import sys
from collections import defaultdict
from pathlib import Path
from statistics import median


def load_results(directory: Path) -> list[dict]:
    """Load all *_run_*.json files from the given directory."""
    results = []
    for entry in sorted(directory.iterdir()):
        if entry.is_file() and entry.name.endswith(".json") and "_run_" in entry.name:
            with open(entry) as f:
                data = json.load(f)
                data["_file"] = entry.name
                results.append(data)
    return results


def group_by_implementation(results: list[dict]) -> dict[str, list[dict]]:
    """Group results by implementation name."""
    groups: dict[str, list[dict]] = defaultdict(list)
    for r in results:
        groups[r["implementation"]].append(r)
    return dict(groups)


def compute_summary(runs: list[dict]) -> dict:
    """Compute median of each metric across runs."""
    keys = [
        "cycles_per_op_p50",
        "cycles_per_op_p99",
        "cycles_per_op_p999",
        "cycles_per_op_p9999",
        "cycles_per_op_max",
        "cycles_per_op_mean",
    ]
    summary = {}
    for k in keys:
        values = [r["results"][k] for r in runs]
        summary[k] = median(values)
    return summary


def fmt_val(v) -> str:
    """Format a numeric value: int if whole, otherwise 1 decimal."""
    if isinstance(v, float) and v != int(v):
        return f"{v:.1f}"
    return str(int(v))


def print_system_info(results: list[dict]) -> None:
    """Print system info header from the first result."""
    r = results[0]
    cpu = r.get("cpu", "unknown")
    cores = f"{r.get('producer_core', '?')} -> {r.get('consumer_core', '?')}"
    msgs = r.get("measured_messages", "?")
    warmup = r.get("warmup_messages", "?")

    print("## System Info\n")
    print(f"- **CPU:** {cpu}")
    print(f"- **Pinned cores:** {cores}")
    print(f"- **Messages per run:** {msgs:,}" if isinstance(msgs, int) else f"- **Messages per run:** {msgs}")
    print(f"- **Warmup messages:** {warmup:,}" if isinstance(warmup, int) else f"- **Warmup messages:** {warmup}")
    print(f"- **Runs per implementation:** {len(results)}")
    print()


def print_summary_table(groups: dict[str, list[dict]]) -> None:
    """Print the summary comparison table sorted by median p50."""
    rows = []
    for impl_name, runs in groups.items():
        summary = compute_summary(runs)
        language = runs[0].get("language", "?")
        rows.append((impl_name, language, summary))

    # Sort by median p50 (fastest first)
    rows.sort(key=lambda r: r[2]["cycles_per_op_p50"])

    print("## Summary (median across runs, cycles/op — lower is better)\n")
    print("| Implementation | Language | p50 | p99 | p99.9 | p99.99 | max | mean |")
    print("|---|---|---:|---:|---:|---:|---:|---:|")

    for impl_name, language, s in rows:
        print(
            f"| {impl_name} | {language} "
            f"| {fmt_val(s['cycles_per_op_p50'])} "
            f"| {fmt_val(s['cycles_per_op_p99'])} "
            f"| {fmt_val(s['cycles_per_op_p999'])} "
            f"| {fmt_val(s['cycles_per_op_p9999'])} "
            f"| {fmt_val(s['cycles_per_op_max'])} "
            f"| {fmt_val(s['cycles_per_op_mean'])} |"
        )
    print()


def print_detail_tables(groups: dict[str, list[dict]]) -> None:
    """Print per-run detail tables for each implementation."""
    for impl_name in sorted(groups):
        runs = groups[impl_name]
        language = runs[0].get("language", "?")
        print(f"### {impl_name} ({language}) — {len(runs)} run(s)\n")
        print("| Run | p50 | p99 | p99.9 | p99.99 | max | min | mean |")
        print("|---|---:|---:|---:|---:|---:|---:|---:|")

        for i, run in enumerate(runs, 1):
            r = run["results"]
            print(
                f"| {i} "
                f"| {fmt_val(r['cycles_per_op_p50'])} "
                f"| {fmt_val(r['cycles_per_op_p99'])} "
                f"| {fmt_val(r['cycles_per_op_p999'])} "
                f"| {fmt_val(r['cycles_per_op_p9999'])} "
                f"| {fmt_val(r['cycles_per_op_max'])} "
                f"| {fmt_val(r['cycles_per_op_min'])} "
                f"| {fmt_val(r['cycles_per_op_mean'])} |"
            )
        print()


def main() -> None:
    if len(sys.argv) != 2:
        print(f"Usage: {sys.argv[0]} <results-directory>", file=sys.stderr)
        sys.exit(1)

    directory = Path(sys.argv[1])
    if not directory.is_dir():
        print(f"Error: {directory} is not a directory", file=sys.stderr)
        sys.exit(1)

    results = load_results(directory)
    if not results:
        print(f"No *_run_*.json files found in {directory}", file=sys.stderr)
        sys.exit(1)

    groups = group_by_implementation(results)

    print("# SPSC Benchmark Comparison\n")
    print_system_info(results)
    print_summary_table(groups)
    print_detail_tables(groups)


if __name__ == "__main__":
    main()
