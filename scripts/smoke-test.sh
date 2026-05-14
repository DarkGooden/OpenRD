#!/usr/bin/env bash
# Smoke test for the hello exchange: build, start server, run test client,
# print server log, exit. Intended to run inside the Rust container.
set -euo pipefail

echo "=== build ==="
cargo build --bins 2>&1 | tail -5

echo "=== launching server ==="
RUST_LOG=openrd_server=info,openrd_proto=info ./target/debug/openrd-server >/tmp/srv.log 2>&1 &
SRV=$!
sleep 1

echo "=== running test client ==="
if ./target/debug/openrd-test-client; then
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
