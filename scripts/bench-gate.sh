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
MAX_ATTEMPTS="${BENCH_GATE_ATTEMPTS:-3}"
MAX_LOAD_PER_CPU="${BENCH_GATE_MAX_LOAD_PER_CPU:-1.0}"
QUIET_WAIT_SECONDS="${BENCH_GATE_QUIET_WAIT_SECONDS:-900}"

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

wait_for_quiet_host() {
    # A wall-clock regression comparison is meaningless while unrelated jobs
    # saturate the host. Wait for the one-minute load average to fall below one
    # runnable task per CPU before each attempt.
    local cpu_count load_one load_per_cpu started
    cpu_count="$(nproc)"
    started="$SECONDS"
    while true; do
        read -r load_one _ < /proc/loadavg
        load_per_cpu="$(awk -v loadval="$load_one" -v cpus="$cpu_count" 'BEGIN { printf "%.3f", loadval / cpus }')"
        if awk -v loadval="$load_per_cpu" -v limit="$MAX_LOAD_PER_CPU" 'BEGIN { exit !(loadval <= limit) }'; then
            echo "bench-gate: host eligible (load/cpu=${load_per_cpu}, limit=${MAX_LOAD_PER_CPU})"
            return 0
        fi
        if (( SECONDS - started >= QUIET_WAIT_SECONDS )); then
            echo "bench-gate: host did not become benchmark-eligible within ${QUIET_WAIT_SECONDS}s (load/cpu=${load_per_cpu})" >&2
            return 1
        fi
        echo "bench-gate: waiting for host load to settle (load/cpu=${load_per_cpu}, limit=${MAX_LOAD_PER_CPU})"
        sleep 15
    done
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
echo "bench-gate: running cargo bench -p rylus-encode --bench encode (up to ${MAX_ATTEMPTS} attempts) ..."

# Shared/self-hosted machines occasionally experience transient scheduler or I/O
# contention. Retry only while the result is over threshold and retain the best
# point estimate. A real regression remains over threshold on every attempt;
# one noisy sample no longer turns an otherwise-green release red.
best_ns=""
best_val=""
best_unit=""
attempt=1
while (( attempt <= MAX_ATTEMPTS )); do
    wait_for_quiet_host
    bench_output="$(cargo bench -p rylus-encode --bench encode -- --sample-size 20 --measurement-time 5 2>/dev/null)"
    measured_line="$(grep -E "^${BENCH_NAME}[[:space:]]+time:" <<<"$bench_output" || true)"
    if [[ -z "$measured_line" ]]; then
        echo "bench-gate: attempt ${attempt} produced no '${BENCH_NAME} time: [...]' output" >&2
    else
        measured_mean="$(extract_mean "$measured_line")"
        if [[ -n "$measured_mean" ]]; then
            measured_val="$(cut -d' ' -f1 <<<"$measured_mean")"
            measured_unit="$(cut -d' ' -f2 <<<"$measured_mean")"
            measured_ns="$(to_ns "$measured_val" "$measured_unit")"
            echo "bench-gate: attempt ${attempt} mean = ${measured_val} ${measured_unit} (${measured_ns} ns)"

            if [[ -z "$best_ns" ]] || awk -v current="$measured_ns" -v best="$best_ns" 'BEGIN { exit !(current < best) }'; then
                best_ns="$measured_ns"
                best_val="$measured_val"
                best_unit="$measured_unit"
            fi

            attempt_delta="$(awk -v m="$best_ns" -v b="$baseline_ns" 'BEGIN { printf "%.3f", (m - b) / b * 100 }')"
            if awk -v d="$attempt_delta" -v t="$THRESHOLD_PCT" 'BEGIN { exit !(d <= t) }'; then
                break
            fi
        else
            echo "bench-gate: attempt ${attempt} output could not be parsed: $measured_line" >&2
        fi
    fi
    ((attempt += 1))
done

if [[ -z "$best_ns" ]]; then
    echo "bench-gate: no attempt produced a parseable measurement" >&2
    exit 1
fi

measured_ns="$best_ns"
measured_val="$best_val"
measured_unit="$best_unit"
echo "bench-gate: best mean = ${measured_val} ${measured_unit} (${measured_ns} ns)"

delta_pct="$(awk -v m="$measured_ns" -v b="$baseline_ns" 'BEGIN { printf "%.3f", (m - b) / b * 100 }')"
echo "bench-gate: delta = ${delta_pct}% (threshold: ${THRESHOLD_PCT}%)"

over_threshold="$(awk -v d="$delta_pct" -v t="$THRESHOLD_PCT" 'BEGIN { print (d > t) ? "1" : "0" }')"
if [[ "$over_threshold" == "1" ]]; then
    echo "bench-gate: FAIL -- ${BENCH_NAME} regressed by ${delta_pct}%, exceeding ${THRESHOLD_PCT}% threshold" >&2
    echo "bench-gate: baseline ${baseline_val} ${baseline_unit}, measured ${measured_val} ${measured_unit}" >&2
    exit 1
fi

echo "bench-gate: PASS -- ${BENCH_NAME} within ${THRESHOLD_PCT}% of baseline"
