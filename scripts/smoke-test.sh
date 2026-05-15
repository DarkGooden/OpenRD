#!/usr/bin/env bash
# Smoke test for the hello exchange + PIN auth: build, start server,
# wait for the server's PIN, run test client, print server log, exit.
# Intended to run inside the Rust container.
set -eu
# Note: deliberately NOT setting pipefail; the grep-on-an-empty-log loop
# below tolerates non-matching pipelines as part of its polling.

echo "=== build ==="
cargo build --bins 2>&1 | tail -5

echo "=== launching server ==="
RUST_LOG=openrd_server=info,openrd_proto=info ./target/debug/openrd-server >/tmp/srv.log 2>&1 &
SRV=$!

# Wait for the server to log its PIN. tracing-subscriber emits ANSI
# escape codes around field names even when stdout is redirected, so
# strip them first.
PIN=""
for _ in $(seq 1 50); do
  PIN="$(sed -e 's/\x1b\[[0-9;]*m//g' /tmp/srv.log 2>/dev/null | grep -oE 'pin=[0-9]+' | head -n1 | cut -d= -f2 || true)"
  if [ -n "$PIN" ]; then
    break
  fi
  sleep 0.1
done
if [ -z "$PIN" ]; then
  echo "FAIL: server did not announce PIN within 5s"
  cat /tmp/srv.log
  kill $SRV 2>/dev/null || true
  exit 1
fi
echo "extracted server PIN: $PIN"

echo "=== running test client ==="
if OPENRD_PIN="$PIN" ./target/debug/openrd-test-client; then
  CLIENT_OK=1
else
  CLIENT_OK=0
fi

sleep 0.2
kill $SRV 2>/dev/null || true
wait $SRV 2>/dev/null || true

echo "=== server log ==="
cat /tmp/srv.log

if [ "$CLIENT_OK" -ne 1 ]; then
  echo "=== SMOKE TEST FAILED (client exited nonzero) ==="
  exit 1
fi

# Verify the H.264 dump.
DUMP=/tmp/openrd-display.h264
if [ -f "$DUMP" ]; then
  SIZE=$(stat -c%s "$DUMP")
  echo "Display dump: $DUMP ($SIZE bytes)"
  if [ "$SIZE" -lt 1024 ]; then
    echo "FAIL: Display dump suspiciously small (<1 KiB)"
    exit 1
  fi
  # First 5 bytes should be 0x00 0x00 0x00 0x01 + a NAL header byte
  HEAD=$(head -c 4 "$DUMP" | od -An -tx1 | tr -d ' \n')
  if [ "$HEAD" != "00000001" ]; then
    echo "FAIL: Display dump doesn't start with Annex-B start code (got $HEAD)"
    exit 1
  fi
  echo "Display dump starts with Annex-B start code: OK"
else
  echo "FAIL: Display dump $DUMP not found"
  exit 1
fi

echo "=== SMOKE TEST PASSED ==="
exit 0
