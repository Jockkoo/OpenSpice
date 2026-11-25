# Run makepkg from the OpenSpice checkout.
pkgname=openspice
pkgver=0.2.0
pkgrel=1
pkgdesc='Native circuit schematic editor and ngspice simulator'
arch=('x86_64')
depends=('glibc' 'libgcc' 'libglvnd' 'libx11' 'libxcb' 'libxkbcommon' 'wayland' 'ngspice' 'xdg-desktop-portal')
makedepends=('cargo')
options=('!lto')

build() {
    cd "$startdir"
    CFLAGS='-march=x86-64 -O2 -pipe' CXXFLAGS='-march=x86-64 -O2 -pipe' \
        RUSTFLAGS='-C target-cpu=x86-64' cargo build --locked --release --target-dir "$srcdir/target"
}

package() {
    install -Dm755 "$srcdir/target/release/openspice" "$pkgdir/usr/bin/openspice"
    install -Dm644 "$startdir/assets/openspice.png" "$pkgdir/usr/share/pixmaps/openspice.png"
    install -Dm644 "$startdir/packaging/arch/openspice.desktop" "$pkgdir/usr/share/applications/openspice.desktop"
    install -Dm644 "$startdir/README.md" "$pkgdir/usr/share/doc/openspice/README.md"
}
