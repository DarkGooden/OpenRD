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

if [ "$CLIENT_OK" -eq 1 ]; then
  echo "=== SMOKE TEST PASSED ==="
  exit 0
else
  echo "=== SMOKE TEST FAILED (client exited nonzero) ==="
  exit 1
fi
