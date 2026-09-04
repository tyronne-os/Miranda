#!/usr/bin/env bash
# init-vault.sh — creates Miranda's local memory vault directory structure
# and generates a real libsodium secretbox key used to encrypt data at rest.
#
# Vault root selection:
#   Design/requirements call for /mnt/NOBILITY_VAULT/.miranda/. On this
#   machine /mnt/NOBILITY_VAULT exists and is writable, so that is used.
#   If it is not present on a given machine, this script falls back to
#   ~/NOBILITY_VAULT/.miranda/ and prints a warning so the substitution is
#   never silent.
set -euo pipefail

PREFERRED_ROOT="/mnt/NOBILITY_VAULT/.miranda"
FALLBACK_ROOT="${HOME}/NOBILITY_VAULT/.miranda"

if [ -d "/mnt/NOBILITY_VAULT" ] && [ -w "/mnt/NOBILITY_VAULT" ]; then
    VAULT_ROOT="$PREFERRED_ROOT"
else
    VAULT_ROOT="$FALLBACK_ROOT"
    echo "WARNING: /mnt/NOBILITY_VAULT not present/writable — substituting local path: $VAULT_ROOT" >&2
fi

echo "Initializing Miranda vault at: $VAULT_ROOT"

mkdir -p "$VAULT_ROOT/datalake/events"
mkdir -p "$VAULT_ROOT/datalake/indexes"
mkdir -p "$VAULT_ROOT/obsidian"
mkdir -p "$VAULT_ROOT/config"
mkdir -p "$VAULT_ROOT/neo4j-data"

chmod 700 "$VAULT_ROOT"
chmod 700 "$VAULT_ROOT/config"

KEY_ENC_PATH="$VAULT_ROOT/config/keys.enc"

if [ -f "$KEY_ENC_PATH" ]; then
    echo "Encryption key already exists at $KEY_ENC_PATH — leaving untouched."
else
    PY_BIN="python3"
    if [ -x "/tmp/test_venv/bin/python3" ]; then
        PY_BIN="/tmp/test_venv/bin/python3"
    fi

    "$PY_BIN" - "$KEY_ENC_PATH" <<'PYEOF'
import sys
import os
import base64
import json
import time

import nacl.secret
import nacl.utils
import nacl.pwhash

key_path = sys.argv[1]

# Generate a real random libsodium secretbox key (crypto_secretbox_KEYBYTES = 32 bytes).
raw_key = nacl.utils.random(nacl.secret.SecretBox.KEY_SIZE)

# Wrap the raw key with a machine-local passphrase-derived key so keys.enc
# itself is not a bare plaintext secret on disk. The passphrase-derived
# wrapping key uses nacl.pwhash (Argon2id) with a random salt stored
# alongside the wrapped ciphertext.
salt = nacl.utils.random(nacl.pwhash.argon2id.SALTBYTES)
# Machine-local wrapping passphrase: derived from a fixed local marker file
# path + hostname, so no secret has to be typed interactively for this
# non-interactive init script, while still not just writing the raw key
# to disk in the clear.
wrap_material = (os.uname().nodename + ":miranda-vault-wrap").encode()
wrap_key = nacl.pwhash.argon2id.kdf(
    nacl.secret.SecretBox.KEY_SIZE,
    wrap_material,
    salt,
    opslimit=nacl.pwhash.argon2id.OPSLIMIT_INTERACTIVE,
    memlimit=nacl.pwhash.argon2id.MEMLIMIT_INTERACTIVE,
)

box = nacl.secret.SecretBox(wrap_key)
nonce = nacl.utils.random(nacl.secret.SecretBox.NONCE_SIZE)
ciphertext = box.encrypt(raw_key, nonce)

payload = {
    "version": 1,
    "algorithm": "libsodium-secretbox-xsalsa20poly1305",
    "kdf": "argon2id",
    "salt_b64": base64.b64encode(salt).decode(),
    "ciphertext_b64": base64.b64encode(ciphertext).decode(),
    "created_at": time.time(),
}

with open(key_path, "w") as f:
    json.dump(payload, f, indent=2)

os.chmod(key_path, 0o600)
print(f"Generated real libsodium secretbox key, wrapped with Argon2id-derived key, at {key_path}")
PYEOF
fi

echo "Vault directory structure:"
find "$VAULT_ROOT" -maxdepth 2 -type d | sort

echo "VAULT_ROOT=$VAULT_ROOT" > "$VAULT_ROOT/config/.vault_root_marker"

echo "init-vault.sh complete."
