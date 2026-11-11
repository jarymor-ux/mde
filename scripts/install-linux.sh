#!/bin/sh
# Install the Linux binary and an application shortcut for the current user.
set -eu

bundle_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
if [ ! -f "$bundle_dir/mde" ]; then
    echo "Run install.sh from the extracted mde-linux-x86_64 archive." >&2
    exit 1
fi
if [ -z "${HOME:-}" ]; then
    echo "HOME must be set." >&2
    exit 1
fi

bin_dir="$HOME/.local/bin"
data_dir="${XDG_DATA_HOME:-$HOME/.local/share}"
case "$data_dir" in
    /*) ;;
    *) echo "XDG_DATA_HOME must be an absolute path." >&2; exit 1 ;;
esac
applications_dir="$data_dir/applications"
mkdir -p "$bin_dir" "$applications_dir"
install -m 755 "$bundle_dir/mde" "$bin_dir/mde"

desktop_file="$applications_dir/MDE.desktop"
cat > "$desktop_file" <<EOF
[Desktop Entry]
Version=1.0
Type=Application
Name=MDE
Comment=Markdown editor
Exec="$bin_dir/mde" %F
Icon=accessories-text-editor
Terminal=false
Categories=Office;TextEditor;Utility;
MimeType=text/markdown;text/x-markdown;
StartupNotify=true
EOF
chmod 644 "$desktop_file"

desktop_dir=""
if command -v xdg-user-dir >/dev/null 2>&1; then
    desktop_dir=$(xdg-user-dir DESKTOP 2>/dev/null || true)
fi
if [ -z "$desktop_dir" ] || [ ! -d "$desktop_dir" ]; then
    for candidate in "$HOME/Desktop" "$HOME/Рабочий стол"; do
        if [ -d "$candidate" ]; then
            desktop_dir="$candidate"
            break
        fi
    done
fi
if [ -n "$desktop_dir" ] && [ -d "$desktop_dir" ]; then
    cp "$desktop_file" "$desktop_dir/MDE.desktop"
    chmod +x "$desktop_dir/MDE.desktop"
    if command -v gio >/dev/null 2>&1; then
        gio set "$desktop_dir/MDE.desktop" metadata::trusted true >/dev/null 2>&1 || true
    fi
    echo "Desktop shortcut installed: $desktop_dir/MDE.desktop"
else
    echo "No desktop folder found; use the MDE entry in the application menu."
fi
echo "MDE installed to $bin_dir/mde"
