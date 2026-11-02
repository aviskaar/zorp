#!/usr/bin/env bash
# Runs a bounded local task through every approval and sandbox
# combination zorp-agent supports, so you can see how each mode
# changes what the agent can do.
set -euo pipefail

cd "$(dirname "$0")"

SANDBOX="${SANDBOX:-workspace}"
AUTO_APPROVE="${AUTO_APPROVE:-0}"
MAX_STEPS="${MAX_STEPS:-5}"

# Parse CLI overrides
while [[ $# -gt 0 ]]; do
    case "$1" in
        --sandbox) SANDBOX="$2"; shift 2 ;;
        --auto-approve) AUTO_APPROVE=1; shift ;;
        --max-steps) MAX_STEPS="$2"; shift 2 ;;
        *) echo "unknown arg: $1"; exit 1 ;;
    esac
done

echo "==> Building zorp-agent ..."
cargo build --manifest-path ../../Cargo.toml -p zorp-agent

# Create a fresh workspace for the demo
DEMO_DIR=$(mktemp -d)
trap 'rm -rf "$DEMO_DIR"' EXIT

mkdir -p "$DEMO_DIR/.zorp/flavors"

# Write two source files the agent will interact with
cat > "$DEMO_DIR/greeting.rs" << 'RUST'
/// A function that prints a greeting message.
pub fn greet(name: &str) -> String {
    format!("Hello, {}! Welcome to Zorp.", name)
}

/// Returns true if the given number is even.
pub fn is_even(n: i64) -> bool {
    n % 2 == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn greet_returns_expected() {
        assert_eq!(greet("world"), "Hello, world! Welcome to Zorp.");
    }

    #[test]
    fn is_even_works() {
        assert!(is_even(4));
        assert!(!is_even(3));
    }
}
RUST

cat > "$DEMO_DIR/README.md" << 'MD'
# Demo Project

Zorp example project. Contains a greeting module with basic
functionality.
MD

# Build an agent flavor for consistent behavior
cat > "$DEMO_DIR/.zorp/flavors/demo.toml" << 'TOML'
name = "Demo Agent"
description = "A test agent for bounded task examples."
max_steps = 10
TOML

echo ""
echo "========================================================"
echo " Demo workspace ready at: $DEMO_DIR"
echo " Max steps: $MAX_STEPS | Sandbox: $SANDBOX | Approve: $AUTO_APPROVE"
echo "========================================================"
echo ""

# Set approval and sandbox flags
APPROVAL_FLAGS=""
if [ "$AUTO_APPROVE" = "1" ]; then
    APPROVAL_FLAGS="--yes"
fi

SANDBOX_FLAGS=""
if [ -n "$SANDBOX" ]; then
    SANDBOX_FLAGS="--sandbox $SANDBOX"
fi

echo "==> Running: zorp-agent --max-steps $MAX_STEPS $APPROVAL_FLAGS $SANDBOX_FLAGS"
echo "    Task: Read the greeting module, write a doc comment for greet"
echo ""

cd "$DEMO_DIR"

exec ../../target/debug/zorp-agent \
    --max-steps "$MAX_STEPS" \
    $APPROVAL_FLAGS \
    $SANDBOX_FLAGS \
    "Read greeting.rs. Add a doc comment to the greet function explaining it takes a name and returns a greeting string. Do not change any existing code."
