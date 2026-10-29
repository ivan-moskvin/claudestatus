#!/bin/sh
# Installs clistatus from the latest release. After that the utility looks
# after itself: clistatus update to update, clistatus uninstall to remove.
#
#   curl -fsSL https://raw.githubusercontent.com/ivan-moskvin/clistatus/main/install.sh | sh
set -eu

REPO="ivan-moskvin/clistatus"
BIN_DIR="${CLISTATUS_BIN_DIR:-$HOME/.local/bin}"

# The messages follow the system language, the same way the binary picks it:
# CLISTATUS_LANG wins, then the locale variables. Every language is one
# branch here — nothing below chooses wording again.
case "${CLISTATUS_LANG:-${LC_ALL:-${LC_MESSAGES:-${LANG:-}}}}" in
    ru*)
        m_unknown_os="Неизвестная система: %s — соберите из исходников.\n"
        m_unknown_arch="Неизвестная архитектура: %s — соберите из исходников.\n"
        m_no_sha="Не нашлось ни sha256sum, ни shasum — нечем проверить загрузку.\n"
        m_downloading="==> Скачиваю %s\n"
        m_no_checksum="В релизе нет контрольной суммы для %s.\n"
        m_bad_checksum="Контрольная сумма не сошлась — загрузка побилась.\n"
        m_installed="==> Поставлен в %s\n"
        ;;
    *)
        m_unknown_os="Unknown system: %s — build it from source.\n"
        m_unknown_arch="Unknown architecture: %s — build it from source.\n"
        m_no_sha="Neither sha256sum nor shasum found — nothing to verify the download with.\n"
        m_downloading="==> Downloading %s\n"
        m_no_checksum="The release has no checksum for %s.\n"
        m_bad_checksum="The checksum does not match — the download is broken.\n"
        m_installed="==> Installed to %s\n"
        ;;
esac

os="$(uname -s | tr '[:upper:]' '[:lower:]')"
case "$os" in
    darwin|linux) ;;
    *) printf "$m_unknown_os" "$os" >&2; exit 1 ;;
esac

case "$(uname -m)" in
    x86_64|amd64) arch=amd64 ;;
    arm64|aarch64) arch=arm64 ;;
    *) printf "$m_unknown_arch" "$(uname -m)" >&2; exit 1 ;;
esac

asset="clistatus_${os}_${arch}"
base="https://github.com/$REPO/releases/latest/download"

# Linux has sha256sum, macOS has shasum.
if command -v sha256sum >/dev/null 2>&1; then
    sha256() { sha256sum "$1" | cut -d' ' -f1; }
elif command -v shasum >/dev/null 2>&1; then
    sha256() { shasum -a 256 "$1" | cut -d' ' -f1; }
else
    printf "$m_no_sha" >&2
    exit 1
fi

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

printf "$m_downloading" "$asset"
curl -fsSL "$base/$asset" -o "$tmp/clistatus"
curl -fsSL "$base/checksums.txt" -o "$tmp/checksums.txt"

want="$(grep " \*\{0,1\}$asset\$" "$tmp/checksums.txt" | cut -d' ' -f1)"
if [ -z "$want" ]; then
    printf "$m_no_checksum" "$asset" >&2
    exit 1
fi
if [ "$want" != "$(sha256 "$tmp/clistatus")" ]; then
    printf "$m_bad_checksum" >&2
    exit 1
fi

mkdir -p "$BIN_DIR"
chmod +x "$tmp/clistatus"
mv "$tmp/clistatus" "$BIN_DIR/clistatus"
printf "$m_installed" "$BIN_DIR/clistatus"

"$BIN_DIR/clistatus" install
