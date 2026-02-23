#!/usr/bin/env bash
set -euo pipefail

REPO="nka11/semantic-code-mcp"
BINARY="semantic-code-mcp"
INSTALL_DIR="${HOME}/.local/bin"

# Use VERSION env var or default to "latest"
VERSION="${VERSION:-latest}"

# Detect OS
OS="$(uname -s)"
case "${OS}" in
  Linux)  OS_TARGET="unknown-linux-gnu" ;;
  Darwin) OS_TARGET="apple-darwin" ;;
  *)
    echo "Error: Unsupported OS '${OS}'. Please download manually from:"
    echo "  https://github.com/${REPO}/releases"
    exit 1
    ;;
esac

# Detect architecture
ARCH="$(uname -m)"
case "${ARCH}" in
  x86_64|amd64)  ARCH_TARGET="x86_64" ;;
  aarch64|arm64)  ARCH_TARGET="aarch64" ;;
  *)
    echo "Error: Unsupported architecture '${ARCH}'. Please download manually from:"
    echo "  https://github.com/${REPO}/releases"
    exit 1
    ;;
esac

TARGET="${ARCH_TARGET}-${OS_TARGET}"
ASSET="${BINARY}-${TARGET}.tar.gz"

if [ "${VERSION}" = "latest" ]; then
  DOWNLOAD_URL="https://github.com/${REPO}/releases/latest/download/${ASSET}"
else
  DOWNLOAD_URL="https://github.com/${REPO}/releases/download/${VERSION}/${ASSET}"
fi

echo "Installing ${BINARY} (${VERSION}) for ${TARGET}..."
echo "Downloading from: ${DOWNLOAD_URL}"

# Create install directory
mkdir -p "${INSTALL_DIR}"

# Download and extract
TMPDIR="$(mktemp -d)"
trap 'rm -rf "${TMPDIR}"' EXIT

if ! curl -fsSL "${DOWNLOAD_URL}" -o "${TMPDIR}/${ASSET}"; then
  echo "Error: Download failed. Check that the release exists:"
  echo "  https://github.com/${REPO}/releases"
  exit 1
fi

tar xzf "${TMPDIR}/${ASSET}" -C "${TMPDIR}"
install -m 755 "${TMPDIR}/${BINARY}" "${INSTALL_DIR}/${BINARY}"

echo ""
echo "Installed ${BINARY} to ${INSTALL_DIR}/${BINARY}"

# Check if install dir is in PATH
if ! echo "${PATH}" | tr ':' '\n' | grep -qx "${INSTALL_DIR}"; then
  echo ""
  echo "WARNING: ${INSTALL_DIR} is not in your PATH."
  echo "Add it with:"
  echo "  export PATH=\"${INSTALL_DIR}:\${PATH}\""
  echo ""
  echo "To make it permanent, add the line above to your ~/.bashrc or ~/.zshrc"
fi

echo ""
echo "Configure in ~/.claude.json:"
echo '  {'
echo '    "mcpServers": {'
echo '      "semantic-code-mcp": {'
echo "        \"command\": \"${INSTALL_DIR}/${BINARY}\""
echo '      }'
echo '    }'
echo '  }'
echo ""
echo "Done!"
