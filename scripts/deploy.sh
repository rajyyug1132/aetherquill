#!/bin/sh
# Copy the built client + run script to the tablet over SSH (USB: 10.11.99.1).
# Run from the repo root, from the environment that built the binary (WSL/Linux).
set -e

RM_HOST="${RM_HOST:-root@10.11.99.1}"
BIN="client/target/armv7-unknown-linux-gnueabihf/release/wha-rm2"

if [ ! -f "$BIN" ]; then
    echo "binary not found — build first:" >&2
    echo "  cd client && cross build --release --target armv7-unknown-linux-gnueabihf" >&2
    exit 1
fi

ssh "$RM_HOST" mkdir -p /home/root/wha
scp "$BIN" scripts/run-on-device.sh "$RM_HOST:/home/root/wha/"
ssh "$RM_HOST" chmod +x /home/root/wha/wha-rm2 /home/root/wha/run-on-device.sh
echo "deployed. start the oracle on this machine (node service/server.js), then:"
echo "  ssh $RM_HOST /home/root/wha/run-on-device.sh"
