#!/usr/bin/env sh
# install.sh — Oximy Gateway installer
#
# Usage:
#   curl -fsSL https://raw.githubusercontent.com/oximyhq/gateway/main/install.sh | sh
#
# What it does:
#   1. Detects OS and architecture.
#   2. Downloads the matching release binary from GitHub Releases.
#   3. Verifies the SHA-256 checksum.
#   4. Installs to ~/.local/bin (or /usr/local/bin with sudo if needed).
#   5. Prints next steps.
#
# Environment variables (all optional):
#   OXIMY_VERSION   — pin to a specific release tag, e.g. v0.1.0
#                     (default: latest)
#   OXIMY_INSTALL_DIR — override installation directory
#   NO_VERIFY       — set to 1 to skip checksum verification (not recommended)

set -eu

REPO="oximyhq/gateway"
BINARY="oximy-gateway"
RELEASES_URL="https://github.com/${REPO}/releases"
API_URL="https://api.github.com/repos/${REPO}/releases/latest"

# ── Helpers ──────────────────────────────────────────────────────────────────

say() { printf "  %s\n" "$@"; }
err() { printf "\nERROR: %s\n\n" "$@" >&2; exit 1; }

need() {
  command -v "$1" >/dev/null 2>&1 || err "'$1' is required but not found in PATH. Please install it and retry."
}

# ── Detect OS ────────────────────────────────────────────────────────────────

detect_os() {
  uname_s="$(uname -s)"
  case "${uname_s}" in
    Linux)  echo "linux" ;;
    Darwin) echo "macos" ;;
    MINGW*|MSYS*|CYGWIN*|Windows_NT)
      err "Windows is not supported by this shell installer. Download the .zip from: ${RELEASES_URL}" ;;
    *)      err "Unsupported operating system: ${uname_s}" ;;
  esac
}

# ── Detect architecture ───────────────────────────────────────────────────────

detect_arch() {
  uname_m="$(uname -m)"
  case "${uname_m}" in
    x86_64|amd64)           echo "x86_64" ;;
    aarch64|arm64)          echo "aarch64" ;;
    armv7l)
      err "32-bit ARM is not supported. Please open an issue if you need it." ;;
    *)  err "Unsupported architecture: ${uname_m}" ;;
  esac
}

# ── Map OS+arch → release target triple ──────────────────────────────────────

target_triple() {
  os="$1"
  arch="$2"
  case "${os}-${arch}" in
    linux-x86_64)   echo "x86_64-unknown-linux-gnu" ;;
    linux-aarch64)  echo "aarch64-unknown-linux-gnu" ;;
    macos-x86_64)   echo "x86_64-apple-darwin" ;;
    macos-aarch64)  echo "aarch64-apple-darwin" ;;
    *)              err "No pre-built binary for ${os}-${arch}. Build from source: https://github.com/${REPO}" ;;
  esac
}

# ── Resolve the install directory ─────────────────────────────────────────────

resolve_install_dir() {
  if [ -n "${OXIMY_INSTALL_DIR:-}" ]; then
    echo "${OXIMY_INSTALL_DIR}"
    return
  fi
  # Prefer ~/.local/bin (no sudo required)
  local_bin="${HOME}/.local/bin"
  if [ -d "${local_bin}" ] || mkdir -p "${local_bin}" 2>/dev/null; then
    echo "${local_bin}"
  else
    echo "/usr/local/bin"
  fi
}

# ── Download a URL to a file ──────────────────────────────────────────────────

download() {
  url="$1"
  dest="$2"
  if command -v curl >/dev/null 2>&1; then
    curl -fsSL --proto '=https' --tlsv1.2 -o "${dest}" "${url}"
  elif command -v wget >/dev/null 2>&1; then
    wget -q -O "${dest}" "${url}"
  else
    err "Neither curl nor wget found. Install one and retry."
  fi
}

# ── Verify SHA-256 checksum ───────────────────────────────────────────────────

verify_checksum() {
  file="$1"
  expected="$2"

  if command -v sha256sum >/dev/null 2>&1; then
    actual="$(sha256sum "${file}" | awk '{print $1}')"
  elif command -v shasum >/dev/null 2>&1; then
    actual="$(shasum -a 256 "${file}" | awk '{print $1}')"
  else
    say "Warning: no sha256sum or shasum found; skipping checksum verification."
    return 0
  fi

  if [ "${actual}" != "${expected}" ]; then
    err "Checksum mismatch for ${file}:\n  expected: ${expected}\n  got:      ${actual}\nAborted."
  fi
}

# ── Fetch latest version from GitHub API ─────────────────────────────────────

fetch_latest_version() {
  if command -v curl >/dev/null 2>&1; then
    curl -fsSL --proto '=https' --tlsv1.2 "${API_URL}" \
      | grep '"tag_name"' \
      | head -1 \
      | sed 's/.*"tag_name": *"\([^"]*\)".*/\1/'
  elif command -v wget >/dev/null 2>&1; then
    wget -q -O - "${API_URL}" \
      | grep '"tag_name"' \
      | head -1 \
      | sed 's/.*"tag_name": *"\([^"]*\)".*/\1/'
  else
    err "Neither curl nor wget found."
  fi
}

# ── Main ─────────────────────────────────────────────────────────────────────

main() {
  say ""
  say "Oximy Gateway Installer"
  say "========================"

  # Determine version to install
  version="${OXIMY_VERSION:-}"
  if [ -z "${version}" ]; then
    say "Resolving latest release..."
    version="$(fetch_latest_version)"
    [ -n "${version}" ] || err "Could not determine latest version. Set OXIMY_VERSION=v0.x.y to pin."
  fi
  say "Version: ${version}"

  # Detect platform
  os="$(detect_os)"
  arch="$(detect_arch)"
  triple="$(target_triple "${os}" "${arch}")"
  say "Platform: ${os} / ${arch} (${triple})"

  # Artifact names
  archive="${BINARY}-${triple}.tar.gz"
  checksum_file="${archive}.sha256"
  base_url="${RELEASES_URL}/download/${version}"

  # Work in a temp directory
  tmpdir="$(mktemp -d)"
  trap 'rm -rf "${tmpdir}"' EXIT

  # Download archive + checksum
  say ""
  say "Downloading ${archive}..."
  download "${base_url}/${archive}" "${tmpdir}/${archive}"

  if [ "${NO_VERIFY:-0}" != "1" ]; then
    say "Downloading checksum..."
    if download "${base_url}/${checksum_file}" "${tmpdir}/${checksum_file}" 2>/dev/null; then
      expected_hash="$(awk '{print $1}' "${tmpdir}/${checksum_file}")"
      say "Verifying checksum..."
      verify_checksum "${tmpdir}/${archive}" "${expected_hash}"
      say "Checksum OK."
    else
      say "Warning: checksum file not found; skipping verification."
    fi
  fi

  # Extract
  say "Extracting..."
  tar xzf "${tmpdir}/${archive}" -C "${tmpdir}"

  # Install
  install_dir="$(resolve_install_dir)"
  say "Installing to ${install_dir}/${BINARY}..."

  install_target="${install_dir}/${BINARY}"

  if [ -w "${install_dir}" ]; then
    cp "${tmpdir}/${BINARY}" "${install_target}"
    chmod 755 "${install_target}"
  else
    say "(${install_dir} requires elevated privileges — using sudo)"
    sudo cp "${tmpdir}/${BINARY}" "${install_target}"
    sudo chmod 755 "${install_target}"
  fi

  # Verify it runs
  if "${install_target}" version >/dev/null 2>&1; then
    installed_version="$("${install_target}" version 2>&1 || true)"
    say ""
    say "Installed: ${installed_version}"
  fi

  # PATH reminder
  case ":${PATH}:" in
    *":${install_dir}:"*) : ;;  # already in PATH
    *)
      say ""
      say "NOTE: ${install_dir} is not in your PATH."
      say "Add this to your shell profile (~/.bashrc, ~/.zshrc, etc.):"
      say ""
      say "  export PATH=\"\$HOME/.local/bin:\$PATH\""
      say ""
      ;;
  esac

  say ""
  say "Done! Next steps:"
  say ""
  say "  # Set at least one provider key, then:"
  say "  export OPENAI_API_KEY=sk-..."
  say "  oximy-gateway up"
  say ""
  say "  # Or with Anthropic:"
  say "  export ANTHROPIC_API_KEY=sk-ant-..."
  say "  oximy-gateway up"
  say ""
  say "  # See all options:"
  say "  oximy-gateway up --help"
  say ""
  say "  # Run headlessly (e.g. on a server):"
  say "  oximy-gateway up --no-open --host 0.0.0.0 --port 8080"
  say ""
  say "Docs: https://github.com/${REPO}"
  say ""
}

main "$@"
