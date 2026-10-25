#!/usr/bin/env bash
# make-bundle.sh <version> — build the static armv7 musl binary and pack the
# release bundle: dist/aetherquill-<version>.tar.gz containing
#   aetherquill-<version>/{aetherquill, external.manifest.json, install.sh, start-xovi.sh}
# Used by .github/workflows/release.yml and runnable locally (any OS with
# rustup + bash + tar). Builds and packs only — never deploys anywhere.
set -euo pipefail

version="${1:?usage: scripts/make-bundle.sh <version>}"
root="$(cd "$(dirname "$0")/.." && pwd)"
target=armv7-unknown-linux-musleabihf
name="aetherquill-$version"

cargo build --release --target "$target" --manifest-path "$root/device/Cargo.toml" --bin aetherquill

stage="$root/dist/$name"
rm -rf "$stage"
mkdir -p "$stage"
cp "$root/device/target/$target/release/aetherquill" "$stage/"
cp "$root/device/appload/aetherquill/external.manifest.json" "$stage/"
cp "$root/device/appload/install.sh" "$stage/"
cp "$root/device/appload/wha/start-xovi.sh" "$stage/"
chmod 755 "$stage/aetherquill" "$stage/install.sh" "$stage/start-xovi.sh"

tar -C "$root/dist" -czf "$root/dist/$name.tar.gz" "$name"
echo "bundle: dist/$name.tar.gz"
