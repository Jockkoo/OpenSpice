#!/usr/bin/env bash
set -euo pipefail

if [[ $# != 2 ]]; then
    echo "Usage: bash $0 <OpenSpice Linux x86-64 binary built for Ubuntu 24.04> <output directory>" >&2
    exit 2
fi

project=$(cd "$(dirname "$0")/../.." && pwd)
version=$(sed -n '/^\[package\]/,/^\[/s/^version = "\([^"]*\)"$/\1/p' "$project/Cargo.toml")
staging=$(mktemp -d)
trap 'rm -rf "$staging"' EXIT

install -Dm755 "$1" "$staging/usr/bin/openspice"
install -Dm644 "$project/assets/openspice.png" "$staging/usr/share/pixmaps/openspice.png"
install -Dm644 "$project/packaging/arch/openspice.desktop" "$staging/usr/share/applications/openspice.desktop"
install -Dm644 "$project/assets/fonts/NotoSans-LICENSE.txt" "$staging/usr/share/doc/openspice/NotoSans-LICENSE.txt"

mkdir "$staging/DEBIAN"
cat > "$staging/DEBIAN/control" <<EOF
Package: openspice
Version: $version-1
Section: electronics
Priority: optional
Architecture: amd64
Maintainer: OpenSpice contributors <Jockkoo@users.noreply.github.com>
Depends: libc6 (>= 2.39), libgcc-s1, libgl1, libegl1, libx11-6, libx11-xcb1, libxcb1, libxkbcommon0, libwayland-client0, libwayland-cursor0, ngspice, xdg-desktop-portal
Homepage: https://github.com/Jockkoo/OpenSpice
Description: Desktop circuit schematic editor and simulator
 OpenSpice edits circuit schematics and uses ngspice to run transient,
 AC, and DC operating-point analyses.
EOF

mkdir -p "$2"
dpkg-deb --root-owner-group --build "$staging" "$2/openspice_${version}-1_amd64.deb"
