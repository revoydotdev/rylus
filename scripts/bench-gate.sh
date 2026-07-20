#!/usr/bin/env bash
# Regression gate for the rylus-encode criterion benchmark (M1.P3.S2.T1).
#
# Runs the `encode_1280x720_bgr0` bench for real, extracts its measured mean
# time from criterion's stdout, compares it against the recorded baseline
# mean in crates/rylus-encode/benches/BASELINE.md, and fails (non-zero exit)
# if the new mean exceeds the baseline by more than THRESHOLD_PCT.
#
# The baseline is read out of BASELINE.md rather than hardcoded here, so the
# two numbers cannot drift apart independently.
set -euo pipefail

BENCH_NAME="encode_1280x720_bgr0"
BASELINE_FILE="$(dirname "$0")/../crates/rylus-encode/benches/BASELINE.md"
THRESHOLD_PCT="${BENCH_GATE_THRESHOLD_PCT:-15}"

# Extracts the middle (point-estimate/mean) value + unit from a criterion
# "<bench-name>    time:   [lo mean hi]" line. Criterion auto-picks units
# (ns/µs/ms/s) per magnitude, so the unit must travel with the number.
extract_mean() {
    # $1 = text to search. Prints "<value> <unit>" or nothing if not found.
    local bracket
    bracket="$(grep -oP '\[[0-9.]+ \S+ [0-9.]+ \S+ [0-9.]+ \S+\]' <<<"$1" | head -n1)"
    [[ -z "$bracket" ]] && return 0
    bracket="${bracket#[}"
    bracket="${bracket%]}"
    awk '{print $3, $4}' <<<"$bracket"
}

to_ns() {
    # $1 = numeric value, $2 = unit -> prints value in nanoseconds.
    awk -v val="$1" -v unit="$2" 'BEGIN {
        if (unit == "ns")            { printf "%.6f", val }
        else if (unit == "us" || unit == "µs") { printf "%.6f", val * 1000 }
        else if (unit == "ms")       { printf "%.6f", val * 1000000 }
        else if (unit == "s")        { printf "%.6f", val * 1000000000 }
        else                          { exit 1 }
    }'
}

if [[ ! -f "$BASELINE_FILE" ]]; then
    echo "bench-gate: baseline file not found at $BASELINE_FILE" >&2
    exit 1
fi

baseline_line="$(grep -E "^${BENCH_NAME}[[:space:]]+time:" "$BASELINE_FILE" || true)"
if [[ -z "$baseline_line" ]]; then
    echo "bench-gate: could not find a '${BENCH_NAME} time: [...]' line in $BASELINE_FILE" >&2
    exit 1
fi

baseline_mean="$(extract_mean "$baseline_line")"
if [[ -z "$baseline_mean" ]]; then
    echo "bench-gate: failed to parse baseline mean from: $baseline_line" >&2
    exit 1
fi
baseline_val="$(cut -d' ' -f1 <<<"$baseline_mean")"
baseline_unit="$(cut -d' ' -f2 <<<"$baseline_mean")"
baseline_ns="$(to_ns "$baseline_val" "$baseline_unit")"

echo "bench-gate: baseline mean = ${baseline_val} ${baseline_unit} (${baseline_ns} ns)"
echo "bench-gate: running cargo bench -p rylus-encode --bench encode ..."

bench_output="$(cargo bench -p rylus-encode --bench encode -- --sample-size 20 --measurement-time 5 2>/dev/null)"

measured_line="$(grep -E "^${BENCH_NAME}[[:space:]]+time:" <<<"$bench_output" || true)"
if [[ -z "$measured_line" ]]; then
    echo "bench-gate: bench produced no '${BENCH_NAME} time: [...]' output -- treating as failure" >&2
    echo "--- full bench output ---" >&2
    echo "$bench_output" >&2
    exit 1
fi

measured_mean="$(extract_mean "$measured_line")"
if [[ -z "$measured_mean" ]]; then
    echo "bench-gate: failed to parse measured mean from: $measured_line" >&2
    exit 1
fi
measured_val="$(cut -d' ' -f1 <<<"$measured_mean")"
measured_unit="$(cut -d' ' -f2 <<<"$measured_mean")"
measured_ns="$(to_ns "$measured_val" "$measured_unit")"

echo "bench-gate: measured mean = ${measured_val} ${measured_unit} (${measured_ns} ns)"

delta_pct="$(awk -v m="$measured_ns" -v b="$baseline_ns" 'BEGIN { printf "%.3f", (m - b) / b * 100 }')"
echo "bench-gate: delta = ${delta_pct}% (threshold: ${THRESHOLD_PCT}%)"

over_threshold="$(awk -v d="$delta_pct" -v t="$THRESHOLD_PCT" 'BEGIN { print (d > t) ? "1" : "0" }')"
if [[ "$over_threshold" == "1" ]]; then
    echo "bench-gate: FAIL -- ${BENCH_NAME} regressed by ${delta_pct}%, exceeding ${THRESHOLD_PCT}% threshold" >&2
    echo "bench-gate: baseline ${baseline_val} ${baseline_unit}, measured ${measured_val} ${measured_unit}" >&2
    exit 1
fi

echo "bench-gate: PASS -- ${BENCH_NAME} within ${THRESHOLD_PCT}% of baseline"
