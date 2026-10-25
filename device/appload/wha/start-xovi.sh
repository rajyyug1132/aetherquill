#!/bin/sh
# start-xovi.sh — bring xovi (and so AppLoad/Aetherquill) up after a reboot.
#
# Lives at /home/root/wha/start-xovi.sh. Touches nothing outside /home/root:
# it only runs xovi's own /home/root/xovi/start, and only when xochitl hasn't
# already got xovi loaded — safe to run twice. Run it by hand over SSH after
# each boot:
#     ssh root@10.11.99.1 /home/root/wha/start-xovi.sh
#
# Why not automatic: true boot-time start needs a systemd unit in /etc
# (Vellum's `tripletap` package does exactly that, power-button triple-press),
# which breaks this project's "everything in /home/root, rollback = rm -rf"
# rule. That trade-off is the owner's call — see README "Known issues".
set -eu

XOVI_START=/home/root/xovi/start

if [ ! -x "$XOVI_START" ]; then
    echo "start-xovi: $XOVI_START not found — install xovi via Vellum first" >&2
    exit 1
fi

pid=$(pidof xochitl 2>/dev/null || true)
if [ -n "$pid" ] && grep -q xovi.so "/proc/$pid/maps" 2>/dev/null; then
    echo "start-xovi: xovi already loaded in xochitl (pid $pid) — nothing to do"
    exit 0
fi

echo "start-xovi: starting xovi (the screen will flicker while xochitl reloads)"
exec "$XOVI_START"
