#!/bin/sh
# Takeover runner (runs ON the reMarkable 2): stops xochitl, runs the
# simulator under the rm2fb client shim, and ALWAYS restores xochitl on exit
# (4+ finger tap in the app, Ctrl-C, a crash, or the watchdog below).
#
# Requires toltec's rm2fb ("display" package): the client draws through the
# RM1-style /dev/fb0 + mxcfb API, which librm2fb_client.so provides on a RM2.
set -u

export WHA_ORACLE_ADDR="${WHA_ORACLE_ADDR:-10.11.99.2:7777}"

HEARTBEAT=/home/root/wha/heartbeat
WATCHDOG_TIMEOUT_S=20 # 4x the app's 5s heartbeat interval

# ponytail: not a real systemd unit — that needs a file under /etc/systemd,
# which contradicts the CEO review's home-dir-only / no-/etc-writes safety
# commitment. A plain backgrounded loop inside this already-home-dir-only
# script gets the same "force-restore xochitl if the app hangs" property
# without touching anything outside /home/root/wha.
watchdog() {
    # give the app a moment to write its first heartbeat before checking
    sleep "$WATCHDOG_TIMEOUT_S"
    while kill -0 "$APP_PID" 2>/dev/null; do
        if [ -f "$HEARTBEAT" ]; then
            age=$(( $(date +%s) - $(date -r "$HEARTBEAT" +%s 2>/dev/null || echo 0) ))
            if [ "$age" -gt "$WATCHDOG_TIMEOUT_S" ]; then
                echo "wha-watchdog: heartbeat stale (${age}s) — killing app" >&2
                kill -TERM "$APP_PID" 2>/dev/null
                break
            fi
        fi
        sleep 5
    done
}

restore() {
    [ -n "${WATCHDOG_PID:-}" ] && kill "$WATCHDOG_PID" 2>/dev/null
    rm -f "$HEARTBEAT"
    systemctl start xochitl
}
trap restore EXIT INT TERM

systemctl stop xochitl
systemctl start rm2fb 2>/dev/null || true
rm -f "$HEARTBEAT"

LD_PRELOAD=/opt/lib/librm2fb_client.so.1 /home/root/wha/wha-rm2 &
APP_PID=$!
watchdog &
WATCHDOG_PID=$!
wait "$APP_PID"
