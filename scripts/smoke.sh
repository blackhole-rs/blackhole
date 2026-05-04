#!/usr/bin/env bash
# Local end-to-end smoke test:
# - starts blackhole-mailbox on 127.0.0.1:4000 (in-memory)
# - starts blackhole-transit on 127.0.0.1:4001
# - sends a small file with the CLI
# - receives it
# - verifies the bytes match
#
# Run with: make smoke

set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
TMP="$(mktemp -d -t blackhole-smoke.XXXXXX)"
PIDS=()

cleanup() {
    for pid in "${PIDS[@]:-}"; do
        kill "$pid" 2>/dev/null || true
        wait "$pid" 2>/dev/null || true
    done
    rm -rf "$TMP"
}
trap cleanup EXIT
# Silence bash job-control "Terminated: 15" notices when we kill our backgrounds.
set +m

cd "$ROOT"

echo "==> building binaries"
cargo build -p blackhole-cli -p blackhole-mailbox -p blackhole-transit >/dev/null

MAILBOX="$ROOT/target/debug/blackhole-mailbox"
TRANSIT="$ROOT/target/debug/blackhole-transit"
CLI="$ROOT/target/debug/blackhole"

echo "==> starting mailbox on 127.0.0.1:4000"
RUST_LOG=warn "$MAILBOX" --listen 127.0.0.1:4000 >"$TMP/mailbox.log" 2>&1 &
PIDS+=($!)

echo "==> starting transit on 127.0.0.1:4001"
RUST_LOG=warn "$TRANSIT" --listen 127.0.0.1:4001 >"$TMP/transit.log" 2>&1 &
PIDS+=($!)

# Wait for both to bind.
for i in $(seq 1 20); do
    if lsof -nP -iTCP:4000 -sTCP:LISTEN >/dev/null 2>&1 && \
       lsof -nP -iTCP:4001 -sTCP:LISTEN >/dev/null 2>&1; then
        break
    fi
    sleep 0.1
    if [ "$i" = "20" ]; then
        echo "servers did not bind in time" >&2
        echo "--- mailbox.log ---" >&2; cat "$TMP/mailbox.log" >&2
        echo "--- transit.log ---" >&2; cat "$TMP/transit.log" >&2
        exit 1
    fi
done

# Test payload.
echo "==> creating payload"
PAYLOAD="$TMP/payload.txt"
date > "$PAYLOAD"
echo "blackhole smoke test, $(uname -a)" >> "$PAYLOAD"
PAYLOAD_HASH="$(shasum -a 256 "$PAYLOAD" | awk '{print $1}')"

# Start sender in background; capture output to extract the wormhole code.
echo "==> sending"
SEND_LOG="$TMP/send.log"
WORMHOLE_MAILBOX_URL="ws://127.0.0.1:4000/v1" \
WORMHOLE_RELAY_URL="tcp://127.0.0.1:4001" \
"$CLI" send --no-qr --force-relay "$PAYLOAD" >"$SEND_LOG" 2>&1 &
SEND_PID=$!
PIDS+=($SEND_PID)

# Wait for the code line to appear.
CODE=""
for i in $(seq 1 50); do
    if grep -qE "code is: " "$SEND_LOG" 2>/dev/null; then
        CODE="$(grep -oE '[0-9]+-[a-z]+-[a-z]+' "$SEND_LOG" | head -1)"
        break
    fi
    sleep 0.2
done
if [ -z "$CODE" ]; then
    echo "could not find wormhole code in send output" >&2
    cat "$SEND_LOG" >&2
    exit 1
fi
echo "    code: $CODE"

# Receive into a fresh dir.
RECV_DIR="$TMP/recv"
mkdir -p "$RECV_DIR"
echo "==> receiving"
(
    cd "$RECV_DIR"
    WORMHOLE_MAILBOX_URL="ws://127.0.0.1:4000/v1" \
    WORMHOLE_RELAY_URL="tcp://127.0.0.1:4001" \
    "$CLI" receive --noconfirm --force-relay "$CODE" >"$TMP/recv.log" 2>&1
)

# Wait for sender to finish.
wait "$SEND_PID" || true

# Verify.
RECV_FILE="$RECV_DIR/$(basename "$PAYLOAD")"
if [ ! -f "$RECV_FILE" ]; then
    echo "received file not found at $RECV_FILE" >&2
    echo "--- recv.log ---" >&2; cat "$TMP/recv.log" >&2
    exit 1
fi

RECV_HASH="$(shasum -a 256 "$RECV_FILE" | awk '{print $1}')"
if [ "$PAYLOAD_HASH" != "$RECV_HASH" ]; then
    echo "hash mismatch:" >&2
    echo "  sent:     $PAYLOAD_HASH" >&2
    echo "  received: $RECV_HASH" >&2
    exit 1
fi

echo "==> ok — bytes match (sha256: $PAYLOAD_HASH)"
