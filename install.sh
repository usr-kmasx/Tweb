#!/usr/bin/env bash
# Instalador do Tweb: dependências por distro + build do fonte + instalação.
# Uso: rode na raiz do repositório como usuário normal (com sudo).
set -euo pipefail

APP_BIN="tweb"
SHIM_BIN="web"
PREFIX="/usr/local/bin"

die() { echo "tweb-install: erro: $*" >&2; exit 1; }
info() { echo "tweb-install: $*"; }

# 0. pré-condições
REPO_ROOT="$(cd "$(dirname "$0")" && pwd)"
[ -f "$REPO_ROOT/Cargo.toml" ] || die "rode na raiz do repositório Tweb (sem Cargo.toml aqui)"
[ -f "$REPO_ROOT/bin/web" ] || [ -L "$REPO_ROOT/bin/web" ] || die "sem bin/web aqui"
[ -f "$REPO_ROOT/src/main.rs" ] || die "sem src/main.rs aqui"
[ "$(id -u)" -ne 0 ] || die "rode como usuário normal com sudo, não como root"
command -v sudo >/dev/null || die "sudo não encontrado"

# 1. detecta distro
[ -f /etc/os-release ] || die "/etc/os-release ausente"
. /etc/os-release
DIST="${ID:-} ${ID_LIKE:-}"
PKG_FAMILY=""
case "$DIST" in
  *arch*|*cachyos*|*endeavouros*|*manjaro*) PKG_FAMILY="arch" ;;
  *debian*|*ubuntu*|*linuxmint*|*pop*) PKG_FAMILY="debian" ;;
  *fedora*|*nobara*) PKG_FAMILY="fedora" ;;
  *) die "distro não suportada (ID=${ID:-?}). Suporte: Arch, Debian/Ubuntu, Fedora." ;;
esac
info "distro: $ID (família $PKG_FAMILY)"

# 2. listas por família
case "$PKG_FAMILY" in
  arch)
    SYS_PKGS="base-devel pkgconf curl git ca-certificates-utils python gtk4 vte4 webkitgtk-6.0 gstreamer gst-plugins-base gst-plugins-good gst-plugins-bad gst-plugins-ugly gst-libav gst-plugin-va glib-networking bubblewrap dconf gsettings-desktop-schemas ttf-dejavu"
    ;;
  debian)
    SYS_PKGS="build-essential pkg-config curl git ca-certificates python3 libgtk-4-dev libgtk-4-1 libvte-2.91-gtk4-dev libvte-2.91-gtk4-0 libwebkitgtk-6.0-dev libwebkitgtk-6.0-4 gstreamer1.0-plugins-base gstreamer1.0-plugins-good gstreamer1.0-plugins-bad gstreamer1.0-plugins-ugly gstreamer1.0-libav gstreamer1.0-vaapi glib-networking bubblewrap dconf-gsettings-backend gsettings-desktop-schemas fonts-dejavu-core"
    ;;
  fedora)
    SYS_PKGS="gcc pkgconf-pkg-config curl git ca-certificates python3 gtk4-devel gtk4 vte291-gtk4-devel vte291-gtk4 webkitgtk6.0-devel webkitgtk6.0 gstreamer1-plugins-base gstreamer1-plugins-good gstreamer1-plugins-bad-free gstreamer1-plugins-ugly-free gstreamer1-vaapi glib-networking bubblewrap dconf gsettings-desktop-schemas dejavu-sans-fonts"
    ;;
esac

# 3. instala pacotes
case "$PKG_FAMILY" in
  arch) sudo pacman -S --needed --noconfirm $SYS_PKGS ;;
  debian) sudo apt-get update && sudo apt-get install -y $SYS_PKGS ;;
  fedora) sudo dnf install -y $SYS_PKGS ;;
esac

# 4. gates de versão mínima (iguais aos features do Cargo.toml)
need_min() {
  local ver
  ver="$(pkg-config --modversion "$1" 2>/dev/null)" || die "pkg-config não achou $1"
  if [ "$(printf '%s\n%s\n' "$2" "$ver" | sort -V | head -n 1)" != "$2" ]; then
    die "$1 $ver abaixo do mínimo $2 (use distro mais recente)"
  fi
  info "$1 $ver ok"
}
need_min gtk4 4.12
need_min webkitgtk-6.0 2.42
pkg-config --exists vte-2.91-gtk4 || die "vte-2.91-gtk4 ausente"
info "vte $(pkg-config --modversion vte-2.91-gtk4) ok"
command -v python3 >/dev/null || die "python3 ausente"

# 5. toolchain rust
if ! command -v cargo >/dev/null; then
  info "instalando rustup..."
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal --default-toolchain stable
  . "$HOME/.cargo/env"
fi

# 6. build
cd "$REPO_ROOT"
cargo build --release --locked

# 7. instala binário + symlink web (mesmo arquivo, multicall por argv[0])
sudo install -Dm755 "$REPO_ROOT/target/release/tweb" "$PREFIX/$APP_BIN"
sudo ln -sf "$PREFIX/$APP_BIN" "$PREFIX/$SHIM_BIN"

# 8. verifica linkage
MISSING="$(ldd "$PREFIX/$APP_BIN" 2>/dev/null | grep 'not found' || true)"
[ -z "$MISSING" ] || die "libs ausentes:$MISSING"

info "ok: $PREFIX/$APP_BIN + $PREFIX/$SHIM_BIN"
info "rode: $APP_BIN"
if [ "$PKG_FAMILY" = "fedora" ]; then
  info "nota: codecs patenteados (libav/ugly completos) exigem RPM Fusion; vídeos abertos rodam com os pacotes -free"
fi
