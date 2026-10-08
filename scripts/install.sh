#!/usr/bin/env bash
# Builds Needle and Thread and installs it for the current user, so it starts from the desktop
# like any other app instead of from `cargo tauri dev`.
#
#   scripts/install.sh              build and install
#   scripts/install.sh --uninstall  take it off again
#
# Everything goes under ~/.local, so nothing needs root and nothing touches the system.

set -euo pipefail

APP_NAME="Needle and Thread"
SLUG="needle-and-thread"
BINARY="needle-desktop"
# Must match the window's WM class, or the taskbar shows it as a second, nameless window.
WM_CLASS="$BINARY"

repo=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
bin_dir="${XDG_BIN_HOME:-$HOME/.local/bin}"
apps_dir="${XDG_DATA_HOME:-$HOME/.local/share}/applications"
icons_dir="${XDG_DATA_HOME:-$HOME/.local/share}/icons/hicolor"
desktop_file="$apps_dir/$SLUG.desktop"

say() { printf '\n\033[1m%s\033[0m\n' "$*"; }
oops() { printf '\n\033[31m%s\033[0m\n' "$*" >&2; exit 1; }

refresh_caches() {
    # Both are best-effort: the menu picks the app up on the next login without them.
    if command -v update-desktop-database >/dev/null 2>&1; then
        update-desktop-database "$apps_dir" >/dev/null 2>&1 || true
    fi
    if command -v gtk-update-icon-cache >/dev/null 2>&1; then
        gtk-update-icon-cache -qtf "$icons_dir" >/dev/null 2>&1 || true
    fi
}

if [ "${1:-}" = "--uninstall" ]; then
    rm -f "$bin_dir/$SLUG" "$desktop_file"
    for size in 32 64 128; do
        rm -f "$icons_dir/${size}x${size}/apps/$SLUG.png"
    done
    refresh_caches
    say "Taken off. Your vault and settings are untouched."
    exit 0
fi

for tool in cargo pnpm; do
    command -v "$tool" >/dev/null 2>&1 || oops "No $tool on PATH. See the README's \"Building\" section."
done
command -v trunk >/dev/null 2>&1 || oops "No trunk on PATH: cargo install --locked trunk"
cargo tauri --version >/dev/null 2>&1 || oops "No tauri-cli: cargo install --locked tauri-cli --version '^2'"

say "Building. The first one takes a few minutes."
# --no-bundle skips the .deb, .rpm and AppImage, which need packaging tools this doesn't.
(cd "$repo/crates/desktop" && cargo tauri build --no-bundle)

# The target folder can be moved elsewhere by .cargo/config.toml, so ask cargo where it is.
target=$(cargo metadata --no-deps --format-version 1 --manifest-path "$repo/Cargo.toml" |
    sed -n 's/.*"target_directory":"\([^"]*\)".*/\1/p')
built="$target/release/$BINARY"
[ -x "$built" ] || oops "Built, but no binary at $built"

mkdir -p "$bin_dir" "$apps_dir"
install -m 755 "$built" "$bin_dir/$SLUG"

for size in 32 64 128; do
    icon="$repo/crates/desktop/src-tauri/icons/${size}x${size}.png"
    if [ -f "$icon" ]; then
        mkdir -p "$icons_dir/${size}x${size}/apps"
        install -m 644 "$icon" "$icons_dir/${size}x${size}/apps/$SLUG.png"
    fi
done

cat > "$desktop_file" <<DESKTOP
[Desktop Entry]
Type=Application
Name=$APP_NAME
Comment=A structure-first writing app for long fiction and nonfiction
Exec=$bin_dir/$SLUG %F
Icon=$SLUG
Terminal=false
Categories=Office;WordProcessor;
StartupWMClass=$WM_CLASS
DESKTOP
chmod 644 "$desktop_file"
refresh_caches

say "Installed."
echo "  Start it from your desktop's menu, or run: $SLUG"
echo "  It opens the vault you used last; the first run asks for one."
case ":$PATH:" in
    *":$bin_dir:"*) ;;
    *) echo
       echo "  $bin_dir isn't on your PATH, so only the menu entry will work."
       echo "  To run it by name as well, add this to your shell's rc file:"
       echo "      export PATH=\"$bin_dir:\$PATH\"" ;;
esac
echo
echo "  To update it later, pull and run this again. To remove it: scripts/install.sh --uninstall"
