#!/bin/sh
# install.sh — install (or remove) Aetherquill on a reMarkable 2.
#
# Run ON THE TABLET from the extracted release bundle:
#     cd aetherquill-<version> && sh install.sh            # install / upgrade
#     sh install.sh --uninstall                            # remove everything
#
# Writes ONLY these two directories — nothing in /etc, no system partition,
# no services. Rollback = `sh install.sh --uninstall` (or rm -rf both) + reboot.
set -eu

APP_DIR=/home/root/xovi/exthome/appload/aetherquill
WHA_DIR=/home/root/wha
HERE=$(cd "$(dirname "$0")" && pwd)

if [ "${1:-}" = "--uninstall" ]; then
    rm -rf "$APP_DIR" "$WHA_DIR"
    echo "aetherquill: removed $APP_DIR and $WHA_DIR (spell log included)"
    exit 0
fi

for f in aetherquill external.manifest.json start-xovi.sh; do
    if [ ! -f "$HERE/$f" ]; then
        echo "aetherquill: $f missing from bundle at $HERE — re-extract the release" >&2
        exit 1
    fi
done

if [ ! -d /home/root/xovi/exthome/appload ]; then
    echo "aetherquill: AppLoad not found (/home/root/xovi/exthome/appload)." >&2
    echo "  Install the runtime first: vellum add xovi xovi-extensions appload" >&2
    exit 1
fi

mkdir -p "$APP_DIR" "$WHA_DIR/spells"
cp "$HERE/aetherquill" "$APP_DIR/aetherquill"
cp "$HERE/external.manifest.json" "$APP_DIR/external.manifest.json"
cp "$HERE/start-xovi.sh" "$WHA_DIR/start-xovi.sh"
chmod 755 "$APP_DIR/aetherquill" "$WHA_DIR/start-xovi.sh"

echo "aetherquill: installed to $APP_DIR"
echo "  spell log: $WHA_DIR/spells/log.jsonl"
echo "  after a reboot, start xovi with: $WHA_DIR/start-xovi.sh"
echo "  then open AppLoad in the tablet UI and launch Aetherquill"
