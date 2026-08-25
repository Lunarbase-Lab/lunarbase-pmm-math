#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd -P)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd -P)"
MANIFEST="$REPO_ROOT/solidity-parity/vector-manifest.json"
FORGE_VERSION_FILE="$REPO_ROOT/solidity-parity/forge-version.txt"
ORACLE_COMMIT_FILE="$REPO_ROOT/solidity-parity/oracle-commit.txt"
RUST_VECTORS="$REPO_ROOT/math/rust/lunarbase-pmm-math"
GO_VECTORS="$REPO_ROOT/math/go/testdata"

for command_name in jq cmp wc tr git; do
  command -v "$command_name" >/dev/null || {
    echo "$command_name is required to verify parity vectors" >&2
    exit 127
  }
done

for required in \
  "$MANIFEST" \
  "$FORGE_VERSION_FILE" \
  "$ORACLE_COMMIT_FILE" \
  "$RUST_VECTORS/deterministic_vectors.jsonl" \
  "$RUST_VECTORS/fuzz_vectors.jsonl" \
  "$GO_VECTORS/deterministic_vectors.jsonl" \
  "$GO_VECTORS/fuzz_vectors.jsonl"
do
  if [[ ! -f "$required" ]]; then
    echo "required parity artifact not found: $required" >&2
    exit 1
  fi
done

sha256_file() {
  if command -v sha256sum >/dev/null; then
    sha256sum "$1" | awk '{print $1}'
  else
    shasum -a 256 "$1" | awk '{print $1}'
  fi
}

manifest_string() {
  jq -er ".$1 | select(type == \"string\" and length > 0)" "$MANIFEST"
}

manifest_uint() {
  jq -er ".$1 | select(type == \"number\" and . >= 0 and floor == .)" "$MANIFEST"
}

schema_version="$(manifest_uint schemaVersion)"
if [[ "$schema_version" != "4" ]]; then
  echo "unsupported parity schema: expected v4, got v$schema_version" >&2
  exit 1
fi
fuzz_runs="$(manifest_uint fuzzRuns)"
deterministic_rows="$(manifest_uint deterministicRows)"
fuzz_rows="$(manifest_uint fuzzRows)"
deterministic_sha256="$(manifest_string deterministicSha256)"
fuzz_sha256="$(manifest_string fuzzSha256)"
oracle_commit="$(manifest_string solidityOracleCommit)"
forge_version="$(manifest_string forgeVersion)"
forge_commit="$(manifest_string forgeCommit)"
pinned_forge_version="$(tr -d '[:space:]' < "$FORGE_VERSION_FILE")"
pinned_oracle_commit="$(tr -d '[:space:]' < "$ORACLE_COMMIT_FILE")"

if [[ "$(jq -er '.solidityOracleDirty | type == "boolean" and . == false' "$MANIFEST")" != "true" ]]; then
  echo "manifest must attest to a clean Solidity oracle" >&2
  exit 1
fi
if [[ ! "$oracle_commit" =~ ^[0-9a-fA-F]{40}$ ]]; then
  echo "manifest Solidity commit is not a full Git SHA: $oracle_commit" >&2
  exit 1
fi
if [[ "$oracle_commit" != "$pinned_oracle_commit" ]]; then
  echo "manifest Solidity commit $oracle_commit does not match pinned $pinned_oracle_commit" >&2
  exit 1
fi
if [[ ! "$forge_commit" =~ ^[0-9a-fA-F]{40}$ ]]; then
  echo "manifest Forge commit is not a full Git SHA: $forge_commit" >&2
  exit 1
fi
if [[ "$forge_version" != "$pinned_forge_version" ]]; then
  echo "manifest Forge version $forge_version does not match pinned $pinned_forge_version" >&2
  exit 1
fi
if (( fuzz_rows != fuzz_runs * 2 )); then
  echo "manifest fuzzRows=$fuzz_rows, expected 2 * fuzzRuns=$((fuzz_runs * 2))" >&2
  exit 1
fi

verify_corpus() {
  local label="$1"
  local path="$2"
  local expected_rows="$3"
  local expected_sha256="$4"
  local actual_rows actual_sha256

  actual_rows="$(wc -l < "$path" | tr -d '[:space:]')"
  if [[ "$actual_rows" != "$expected_rows" ]]; then
    echo "$label row count mismatch: expected $expected_rows, got $actual_rows" >&2
    exit 1
  fi
  actual_sha256="$(sha256_file "$path")"
  if [[ "$actual_sha256" != "$expected_sha256" ]]; then
    echo "$label SHA-256 mismatch: expected $expected_sha256, got $actual_sha256" >&2
    exit 1
  fi
  if ! jq -s -e --argjson schema "$schema_version" \
    'length > 0 and all(.[];
      .schemaVersion == $schema
      and (.effectiveFeeX24 | (type == "string") and test("^(0|[1-9][0-9]*)$"))
    )' "$path" >/dev/null
  then
    echo "$label contains invalid JSON, inconsistent schema, or non-canonical effectiveFeeX24" >&2
    exit 1
  fi
}

verify_corpus \
  "Rust deterministic corpus" \
  "$RUST_VECTORS/deterministic_vectors.jsonl" \
  "$deterministic_rows" \
  "$deterministic_sha256"
verify_corpus \
  "Rust fuzz corpus" \
  "$RUST_VECTORS/fuzz_vectors.jsonl" \
  "$fuzz_rows" \
  "$fuzz_sha256"

if ! cmp -s \
  "$RUST_VECTORS/deterministic_vectors.jsonl" \
  "$GO_VECTORS/deterministic_vectors.jsonl"
then
  echo "Go deterministic corpus is not byte-identical to the Rust/Node canonical copy" >&2
  exit 1
fi
if ! cmp -s \
  "$RUST_VECTORS/fuzz_vectors.jsonl" \
  "$GO_VECTORS/fuzz_vectors.jsonl"
then
  echo "Go fuzz corpus is not byte-identical to the Rust/Node canonical copy" >&2
  exit 1
fi

DARK_POOLS_INPUT="${DARK_POOLS_DIR:-${1:-}}"
if [[ -n "$DARK_POOLS_INPUT" ]]; then
  if [[ -f "$DARK_POOLS_INPUT/contracts/src/Pool.sol" ]]; then
    DARK_CONTRACTS="$(cd "$DARK_POOLS_INPUT/contracts" && pwd -P)"
  elif [[ -f "$DARK_POOLS_INPUT/src/Pool.sol" ]]; then
    DARK_CONTRACTS="$(cd "$DARK_POOLS_INPUT" && pwd -P)"
  else
    echo "dark_pools contracts not found below: $DARK_POOLS_INPUT" >&2
    exit 2
  fi

  actual_commit="$(git -C "$DARK_CONTRACTS" rev-parse HEAD)"
  if [[ "$actual_commit" != "$oracle_commit" ]]; then
    echo "Solidity oracle commit mismatch: expected $oracle_commit, got $actual_commit" >&2
    exit 1
  fi
  if [[ -n "$(git -C "$DARK_CONTRACTS" status --porcelain)" ]]; then
    echo "Solidity oracle worktree is dirty: $DARK_CONTRACTS" >&2
    exit 1
  fi

  for entry in \
    "swapLibSha256:src/libraries/SwapLib.sol" \
    "peripheryLibSha256:src/libraries/PeripheryLib.sol" \
    "poolSha256:src/Pool.sol"
  do
    manifest_key="${entry%%:*}"
    relative_path="${entry#*:}"
    expected_hash="$(manifest_string "$manifest_key")"
    actual_hash="$(sha256_file "$DARK_CONTRACTS/$relative_path")"
    if [[ "$actual_hash" != "$expected_hash" ]]; then
      echo "$relative_path SHA-256 mismatch: expected $expected_hash, got $actual_hash" >&2
      exit 1
    fi
  done
fi

echo "Verified schema-v$schema_version Solidity parity corpus: $deterministic_rows deterministic + $fuzz_rows fuzz rows"
echo "Oracle: $oracle_commit; Forge: $forge_version ($forge_commit)"
