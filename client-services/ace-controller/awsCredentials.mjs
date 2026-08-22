/**
 * WO-2 Pipeline 1 — AWS credential retrieval from the AMANDA Access vault.
 *
 * transcribeBridge.mjs and bedrockRouter.mjs deliberately take pre-built
 * AWS SDK clients as arguments and contain no credential-fetching logic
 * themselves (that's what makes them testable with plain mocks, no AWS
 * access required). This module is where real credentials actually get
 * resolved, at the one real call site: run.mjs, right before constructing
 * the real TranscribeStreamingClient / BedrockRuntimeClient.
 *
 * Mechanism: `ace-controller` is a standalone Node process, not an MCP
 * client — it cannot speak the stdio JSON-RPC protocol Kiro uses to reach
 * `~/.kiro/settings/mcp.json`'s `amanda-access-vault` server directly. That
 * MCP server is itself just a thin read-only wrapper over one SQLite
 * table (see /home/hunt/Downloads/THECODE/amanda/kiro-vault-mcp.mjs), so
 * this module opens the same database, read-only, the same way. This is
 * NOT a second vault — it is the one vault, read via a second (equally
 * read-only) path, because the MCP transport itself isn't reachable from
 * a plain Node script.
 *
 * Vault provider names: `aws_access_key_id`, `aws_secret_access_key` — two
 * separate entries, not one combined `aws` entry.
 */

// Reuse AMANDA's pre-built better-sqlite3 native module rather than rebuilding
import { createRequire } from "module";
const require = createRequire(import.meta.url);
const Database = require("/home/hunt/Downloads/THECODE/amanda/node_modules/better-sqlite3");

const VAULT_DB_PATH = "/mnt/NOBILITY_VAULT/amanda-data/db/amanda.db";

/**
 * Reads one provider's key value directly from the vault's SQLite table.
 * Opens the connection read-only and closes it immediately after the
 * query — this function is called at most twice per process (Transcribe +
 * Bedrock credential fetch), so there's no benefit to holding a
 * long-lived handle, and closing eagerly means a vault file that's
 * temporarily locked by another writer doesn't leave this process holding
 * a stale connection.
 *
 * Exported (despite this file's name being AWS-specific) so other modules
 * needing a single vault key — e.g. the NVIDIA NIM key used by the T4
 * cognitive-core pivot in run.mjs — reuse this exact mechanism rather than
 * re-implementing the same SQLite read a second time.
 */
export function readVaultKey(provider) {
  const db = new Database(VAULT_DB_PATH, { readonly: true });
  try {
    const row = db
      .prepare("SELECT key_value FROM keys WHERE provider = ? ORDER BY created_at DESC LIMIT 1")
      .get(provider);
    return row?.key_value ?? null;
  } finally {
    db.close();
  }
}

/**
 * Fetches both AWS credential halves from the vault and returns them
 * shaped as the AWS SDK v3 `credentials` client-config option. Throws with
 * a clear message if either vault entry is missing, rather than silently
 * falling through to the SDK's default credential chain (env vars,
 * ~/.aws/credentials, instance profile) — a silent fallback here would
 * hide exactly the kind of "which credential source is actually active"
 * confusion this Work Order already hit once with the .env file.
 */
export function loadAwsCredentials() {
  const accessKeyId = readVaultKey("aws_access_key_id");
  const secretAccessKey = readVaultKey("aws_secret_access_key");

  if (!accessKeyId || !secretAccessKey) {
    throw new Error(
      "AWS credentials missing from AMANDA vault — expected both " +
        "aws_access_key_id and aws_secret_access_key entries. " +
        "Check the Access panel, not .env.",
    );
  }

  return {
    credentials: { accessKeyId, secretAccessKey },
    region: process.env.AWS_REGION || process.env.AWS_DEFAULT_REGION || "us-east-1",
  };
}
