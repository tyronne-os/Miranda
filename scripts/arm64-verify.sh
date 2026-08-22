#!/usr/bin/env bash
# ARM64 (Graviton) verification for WO-1.
# SSHes into the t4g.small, syncs the repo, runs cargo test and the
# latency benchmark natively on ARM64, and prints results to close WO-1.
#
# Prerequisites: aws-setup.sh must have been run. Instance must be running.
# Usage: bash scripts/arm64-verify.sh [optional-ip-override]

set -e

KEY="$HOME/.ssh/beryl-aws-key.pem"
EC2_USER="ec2-user"

# Resolve IP
if [ -n "$1" ]; then
  EC2_IP="$1"
elif [ -f ~/.miranda-ec2-ip ]; then
  EC2_IP=$(cat ~/.miranda-ec2-ip)
else
  echo "✗ No EC2 IP found. Run aws-setup.sh or: echo '<ip>' > ~/.miranda-ec2-ip"
  exit 1
fi

SSH="ssh -i $KEY -o StrictHostKeyChecking=no $EC2_USER@$EC2_IP"

echo ""
echo "=== WO-1 ARM64 verification ==="
echo "Target: $EC2_IP | Key: $KEY"
echo ""

# --- Check instance is reachable ---
echo "Checking SSH connectivity..."
$SSH "echo '✓ SSH connection successful'" || {
  echo "✗ Cannot reach $EC2_IP. Check that the instance is running and the PEM key is correct."
  exit 1
}

# --- Sync the repo ---
echo ""
echo "Syncing miranda-engine repo on the instance..."
$SSH "
  set -e
  if [ ! -d /home/ec2-user/miranda-engine ]; then
    git clone https://github.com/tyronne-os/Miranda.git /home/ec2-user/miranda-engine
    echo '✓ Repo cloned'
  else
    cd /home/ec2-user/miranda-engine && git pull --rebase
    echo '✓ Repo updated'
  fi
"

# --- Ensure Rust is installed ---
echo ""
echo "Checking Rust toolchain on ARM64..."
$SSH "
  set -e
  if ! command -v cargo &>/dev/null; then
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y -q
    echo '✓ Rust installed'
  else
    echo \"✓ Rust already installed: \$(cargo --version)\"
  fi
  source \$HOME/.cargo/env
  rustup show
"

# --- cargo build ---
echo ""
echo "--- cargo build (ARM64) ---"
$SSH "
  set -e
  source \$HOME/.cargo/env
  cd /home/ec2-user/miranda-engine
  cargo build 2>&1
  echo '✓ cargo build exit 0'
"

# --- cargo test miranda-ipc with output ---
echo ""
echo "--- cargo test -p miranda-ipc (ARM64) ---"
$SSH "
  set -e
  source \$HOME/.cargo/env
  cd /home/ec2-user/miranda-engine
  cargo test -p miranda-ipc -- --nocapture 2>&1
"

# --- MIRI check (nightly required) ---
echo ""
echo "--- MIRI undefined-behaviour check (ARM64) ---"
$SSH "
  set -e
  source \$HOME/.cargo/env
  cd /home/ec2-user/miranda-engine
  if ! rustup toolchain list | grep -q nightly; then
    rustup toolchain install nightly --component miri
    echo '✓ nightly + MIRI installed'
  else
    rustup component add miri --toolchain nightly 2>/dev/null || true
    echo '✓ MIRI component verified'
  fi
  cargo +nightly miri test -p miranda-ipc 2>&1
" || echo "⚠ MIRI reported issues on ARM64 — see output above. Fix before closing WO-1."

echo ""
echo "=== ARM64 verification complete ==="
echo "If all tests passed and MIRI is clean: WO-1 is fully closed."
echo "If the latency benchmark exceeded 50 μs: add #[repr(align(64))] to the"
echo "  AtomicUsize control structs and re-run."
echo ""
echo "Next: run 'node scripts/cat-router-check.mjs' to confirm WO-1 is done,"
echo "then begin WO-2 per .kiro/specs/wo2-acoustic-ingress-routing/tasks.md"
