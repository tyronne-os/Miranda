#!/usr/bin/env bash
# vault-health-check.sh — verifies the Miranda vault directory structure
# exists, is writable, and that the encryption key can be decrypted with
# the same machine-local wrapping key derivation used by init-vault.sh.
set -euo pipefail

PREFERRED_ROOT="/mnt/NOBILITY_VAULT/.miranda"
FALLBACK_ROOT="${HOME}/NOBILITY_VAULT/.miranda"

if [ -d "$PREFERRED_ROOT" ]; then
    VAULT_ROOT="$PREFERRED_ROOT"
elif [ -d "$FALLBACK_ROOT" ]; then
    VAULT_ROOT="$FALLBACK_ROOT"
else
    echo "FAIL: no vault found at $PREFERRED_ROOT or $FALLBACK_ROOT" >&2
    exit 1
fi

echo "Checking vault at: $VAULT_ROOT"

STATUS=0

check_dir() {
    if [ -d "$1" ]; then
        echo "  OK   dir exists: $1"
    else
        echo "  FAIL dir missing: $1"
        STATUS=1
    fi
}

check_dir "$VAULT_ROOT/datalake/events"
check_dir "$VAULT_ROOT/datalake/indexes"
check_dir "$VAULT_ROOT/obsidian"
check_dir "$VAULT_ROOT/config"

# Writability check: write + remove a probe file.
PROBE="$VAULT_ROOT/.health_probe_$$"
if touch "$PROBE" 2>/dev/null; then
    rm -f "$PROBE"
    echo "  OK   vault root is writable"
else
    echo "  FAIL vault root is not writable"
    STATUS=1
fi

KEY_ENC_PATH="$VAULT_ROOT/config/keys.enc"
if [ -f "$KEY_ENC_PATH" ]; then
    echo "  OK   keys.enc present"

    PY_BIN="python3"
    if [ -x "/tmp/test_venv/bin/python3" ]; then
        PY_BIN="/tmp/test_venv/bin/python3"
    fi

    if "$PY_BIN" - "$KEY_ENC_PATH" <<'PYEOF'
import sys, json, base64, os
import nacl.secret, nacl.pwhash

key_path = sys.argv[1]
with open(key_path) as f:
    payload = json.load(f)

salt = base64.b64decode(payload["salt_b64"])
ciphertext = base64.b64decode(payload["ciphertext_b64"])

wrap_material = (os.uname().nodename + ":miranda-vault-wrap").encode()
wrap_key = nacl.pwhash.argon2id.kdf(
    nacl.secret.SecretBox.KEY_SIZE,
    wrap_material,
    salt,
    opslimit=nacl.pwhash.argon2id.OPSLIMIT_INTERACTIVE,
    memlimit=nacl.pwhash.argon2id.MEMLIMIT_INTERACTIVE,
)
box = nacl.secret.SecretBox(wrap_key)
raw_key = box.decrypt(ciphertext)
assert len(raw_key) == nacl.secret.SecretBox.KEY_SIZE
print("  OK   keys.enc decrypts successfully, raw key is", len(raw_key), "bytes")
PYEOF
    then
        :
    else
        echo "  FAIL keys.enc failed to decrypt"
        STATUS=1
    fi
else
    echo "  FAIL keys.enc missing"
    STATUS=1
fi

if [ "$STATUS" -eq 0 ]; then
    echo "Vault health check: PASS"
else
    echo "Vault health check: FAIL"
fi

exit "$STATUS"
