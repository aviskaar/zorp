#!/usr/bin/env bash
set -e

# Build only the two binaries that get installed. Building the whole
# workspace would also compile zorp-track (bundled DuckDB) and zorp-eval,
# neither of which is installed, and a cargo clean here would throw away
# the entire build cache and turn every install into a from-scratch build.
echo "Building the zorp binaries..."
if [ -f Cargo.lock ]; then
    cargo build --release --locked -p zorp -p zorp-agent
else
    cargo build --release -p zorp -p zorp-agent
fi

# Define the target installation directory
# We'll use ~/.local/bin as it's a standard user-level bin directory
INSTALL_DIR="$HOME/.local/bin"

echo "Creating $INSTALL_DIR if it doesn't exist..."
mkdir -p "$INSTALL_DIR"

# install(1) replaces the file atomically, which avoids ETXTBSY when
# upgrading over a binary that is currently running.
echo "Installing binaries to $INSTALL_DIR..."
install -m 755 target/release/zorp "$INSTALL_DIR/"
install -m 755 target/release/zorp-agent "$INSTALL_DIR/"

echo "Successfully installed 'zorp' and 'zorp-agent' to $INSTALL_DIR!"

# Check if INSTALL_DIR is in PATH
if [[ ":$PATH:" != *":$INSTALL_DIR:"* ]]; then
    echo ""
    echo "======================================================================"
    echo "WARNING: $INSTALL_DIR is not in your PATH."
    echo "To attach these binaries to your PATH, add this line to your ~/.zshrc or ~/.bashrc:"
    echo ""
    echo "export PATH=\"\$PATH:$INSTALL_DIR\""
    echo "======================================================================"
    echo ""
    echo "After adding, run 'source ~/.zshrc' (or your shell config) to apply."
else
    echo "The binaries are already accessible in your PATH."
fi
