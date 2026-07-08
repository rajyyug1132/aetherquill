#!/bin/sh
# Takeover runner (runs ON the reMarkable 2): stops xochitl, runs the
# simulator under the rm2fb client shim, and ALWAYS restores xochitl on exit
# (4+ finger tap in the app, Ctrl-C, or a crash).
#
# Requires toltec's rm2fb ("display" package): the client draws through the
# RM1-style /dev/fb0 + mxcfb API, which librm2fb_client.so provides on a RM2.
set -u

export WHA_ORACLE_ADDR="${WHA_ORACLE_ADDR:-10.11.99.2:7777}"

restore() { systemctl start xochitl; }
trap restore EXIT INT TERM

systemctl stop xochitl
systemctl start rm2fb 2>/dev/null || true

LD_PRELOAD=/opt/lib/librm2fb_client.so.1 /home/root/wha/wha-rm2
