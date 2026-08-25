#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd -P)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd -P)"
PARITY_ROOT="$REPO_ROOT/solidity-parity"
FORGE_VERSION_FILE="$PARITY_ROOT/forge-version.txt"
ORACLE_COMMIT_FILE="$PARITY_ROOT/oracle-commit.txt"

DARK_POOLS_INPUT="${DARK_POOLS_DIR:-${1:-}}"
if [[ -z "$DARK_POOLS_INPUT" ]]; then
  echo "usage: DARK_POOLS_DIR=/path/to/dark_pools $0" >&2
  echo "   or: $0 /path/to/dark_pools" >&2
  exit 2
fi

if [[ -f "$DARK_POOLS_INPUT/contracts/src/Pool.sol" ]]; then
  DARK_CONTRACTS="$(cd "$DARK_POOLS_INPUT/contracts" && pwd -P)"
elif [[ -f "$DARK_POOLS_INPUT/src/Pool.sol" ]]; then
  DARK_CONTRACTS="$(cd "$DARK_POOLS_INPUT" && pwd -P)"
else
  echo "dark_pools contracts not found below: $DARK_POOLS_INPUT" >&2
  exit 2
fi

for required in \
  "$DARK_CONTRACTS/foundry.toml" \
  "$DARK_CONTRACTS/src/Pool.sol" \
  "$DARK_CONTRACTS/src/libraries/SwapLib.sol" \
  "$FORGE_VERSION_FILE" \
  "$ORACLE_COMMIT_FILE"
do
  if [[ ! -f "$required" ]]; then
    echo "required Solidity source not found: $required" >&2
    exit 2
  fi
done

command -v forge >/dev/null || {
  echo "forge is required to generate Solidity parity vectors" >&2
  exit 127
}
command -v jq >/dev/null || {
  echo "jq is required to validate Solidity parity vectors" >&2
  exit 127
}

FORGE_VERSION_OUTPUT="$(forge --version)"
FORGE_VERSION="$(awk '/^forge Version:/ {print $3}' <<< "$FORGE_VERSION_OUTPUT")"
FORGE_COMMIT="$(awk '/^Commit SHA:/ {print $3}' <<< "$FORGE_VERSION_OUTPUT")"
PINNED_FORGE_VERSION="$(tr -d '[:space:]' < "$FORGE_VERSION_FILE")"
if [[ "$FORGE_VERSION" != "$PINNED_FORGE_VERSION" ]]; then
  echo "forge $PINNED_FORGE_VERSION is required; found $FORGE_VERSION" >&2
  exit 1
fi
if [[ ! "$FORGE_COMMIT" =~ ^[0-9a-fA-F]{40}$ ]]; then
  echo "forge did not report a full build commit: $FORGE_COMMIT" >&2
  exit 1
fi

ORACLE_COMMIT="$(git -C "$DARK_CONTRACTS" rev-parse HEAD)"
PINNED_ORACLE_COMMIT="$(tr -d '[:space:]' < "$ORACLE_COMMIT_FILE")"
if [[ "$ORACLE_COMMIT" != "$PINNED_ORACLE_COMMIT" ]]; then
  echo "Solidity oracle must be pinned commit $PINNED_ORACLE_COMMIT; found $ORACLE_COMMIT" >&2
  exit 1
fi
if [[ -n "$(git -C "$DARK_CONTRACTS" status --porcelain)" ]]; then
  echo "Solidity oracle worktree must be clean: $DARK_CONTRACTS" >&2
  exit 1
fi

sha256_file() {
  if command -v sha256sum >/dev/null; then
    sha256sum "$1" | awk '{print $1}'
  else
    shasum -a 256 "$1" | awk '{print $1}'
  fi
}

remappings=()
while IFS= read -r mapping; do
  [[ -z "$mapping" ]] && continue
  prefix="${mapping%%=*}"
  target="${mapping#*=}"
  if [[ "$target" != /* ]]; then
    target="$DARK_CONTRACTS/$target"
  fi
  remappings+=("$prefix=$target")
done < <(cd "$DARK_CONTRACTS" && forge remappings)

if [[ ${#remappings[@]} -eq 0 ]]; then
  echo "forge returned no remappings for: $DARK_CONTRACTS" >&2
  exit 2
fi

forge_common=(--root "$PARITY_ROOT")
for mapping in "${remappings[@]}"; do
  forge_common+=(-R "$mapping")
done

mkdir -p "$PARITY_ROOT/generated"
: > "$PARITY_ROOT/generated/deterministic_vectors.jsonl"
: > "$PARITY_ROOT/generated/fuzz_vectors.jsonl"

echo "Generating deterministic Solidity vectors from $DARK_CONTRACTS"
forge test "${forge_common[@]}" \
  --match-contract PoolDeterministicParityOracle \
  --threads 1

FUZZ_RUNS="${PARITY_FUZZ_RUNS:-5000}"
FUZZ_SEED="${PARITY_FUZZ_SEED:-0x706d6d2d7061726974792d763200000000000000000000000000000000000001}"
if [[ ! "$FUZZ_RUNS" =~ ^[1-9][0-9]*$ ]]; then
  echo "PARITY_FUZZ_RUNS must be a positive integer: $FUZZ_RUNS" >&2
  exit 2
fi

echo "Generating $((FUZZ_RUNS * 2)) seeded Solidity fuzz rows"
forge test "${forge_common[@]}" \
  --match-contract PoolFuzzParityOracle \
  --fuzz-runs "$FUZZ_RUNS" \
  --fuzz-seed "$FUZZ_SEED" \
  --threads 1

DETERMINISTIC_SOURCE="$PARITY_ROOT/generated/deterministic_vectors.jsonl"
FUZZ_SOURCE="$PARITY_ROOT/generated/fuzz_vectors.jsonl"
if [[ ! -s "$DETERMINISTIC_SOURCE" ]]; then
  echo "deterministic Solidity corpus is empty: $DETERMINISTIC_SOURCE" >&2
  exit 1
fi
if [[ ! -s "$FUZZ_SOURCE" ]]; then
  echo "fuzz Solidity corpus is empty: $FUZZ_SOURCE" >&2
  exit 1
fi

actual_deterministic_rows="$(wc -l < "$DETERMINISTIC_SOURCE" | tr -d '[:space:]')"
expected_deterministic_rows=49
if [[ "$actual_deterministic_rows" != "$expected_deterministic_rows" ]]; then
  echo "expected $expected_deterministic_rows deterministic rows, generated $actual_deterministic_rows" >&2
  exit 1
fi

actual_fuzz_rows="$(wc -l < "$FUZZ_SOURCE" | tr -d '[:space:]')"
expected_fuzz_rows="$((FUZZ_RUNS * 2))"
if [[ "$actual_fuzz_rows" != "$expected_fuzz_rows" ]]; then
  echo "expected $expected_fuzz_rows fuzz rows, generated $actual_fuzz_rows" >&2
  exit 1
fi

CORPUS_SCHEMA_VERSION="$(head -n 1 "$DETERMINISTIC_SOURCE" | jq -er '.schemaVersion')"
if [[ "$CORPUS_SCHEMA_VERSION" != "4" ]]; then
  echo "expected schema v4 deterministic corpus, generated v$CORPUS_SCHEMA_VERSION" >&2
  exit 1
fi
if ! jq -s -e --argjson schema "$CORPUS_SCHEMA_VERSION" \
  'length > 0 and all(.[]; .schemaVersion == $schema)' "$FUZZ_SOURCE" >/dev/null
then
  echo "fuzz corpus contains invalid JSON or mixed schema versions" >&2
  exit 1
fi

RUST_VECTORS="$REPO_ROOT/math/rust/lunarbase-pmm-math"
GO_VECTORS="$REPO_ROOT/math/go/testdata"
cp "$DETERMINISTIC_SOURCE" "$RUST_VECTORS/deterministic_vectors.jsonl"
cp "$FUZZ_SOURCE" "$RUST_VECTORS/fuzz_vectors.jsonl"
cp "$DETERMINISTIC_SOURCE" "$GO_VECTORS/deterministic_vectors.jsonl"
cp "$FUZZ_SOURCE" "$GO_VECTORS/fuzz_vectors.jsonl"

SWAP_LIB_SHA256="$(sha256_file "$DARK_CONTRACTS/src/libraries/SwapLib.sol")"
PERIPHERY_LIB_SHA256="$(sha256_file "$DARK_CONTRACTS/src/libraries/PeripheryLib.sol")"
POOL_SHA256="$(sha256_file "$DARK_CONTRACTS/src/Pool.sol")"
DETERMINISTIC_SHA256="$(sha256_file "$DETERMINISTIC_SOURCE")"
FUZZ_SHA256="$(sha256_file "$FUZZ_SOURCE")"
MANIFEST="$PARITY_ROOT/vector-manifest.json"
printf '%s\n' \
  '{' \
  "  \"schemaVersion\": $CORPUS_SCHEMA_VERSION," \
  "  \"solidityOracleCommit\": \"$ORACLE_COMMIT\"," \
  '  "solidityOracleDirty": false,' \
  "  \"forgeVersion\": \"$FORGE_VERSION\"," \
  "  \"forgeCommit\": \"$FORGE_COMMIT\"," \
  "  \"swapLibSha256\": \"$SWAP_LIB_SHA256\"," \
  "  \"peripheryLibSha256\": \"$PERIPHERY_LIB_SHA256\"," \
  "  \"poolSha256\": \"$POOL_SHA256\"," \
  "  \"fuzzSeed\": \"$FUZZ_SEED\"," \
  "  \"fuzzRuns\": $FUZZ_RUNS," \
  "  \"deterministicRows\": $actual_deterministic_rows," \
  "  \"fuzzRows\": $actual_fuzz_rows," \
  "  \"deterministicSha256\": \"$DETERMINISTIC_SHA256\"," \
  "  \"fuzzSha256\": \"$FUZZ_SHA256\"" \
  '}' > "$MANIFEST"

echo "Wrote canonical corpora:"
echo "  $RUST_VECTORS/deterministic_vectors.jsonl"
echo "  $RUST_VECTORS/fuzz_vectors.jsonl"
echo "  $GO_VECTORS/deterministic_vectors.jsonl"
echo "  $GO_VECTORS/fuzz_vectors.jsonl"
echo "  $MANIFEST"

bash "$SCRIPT_DIR/verify-vectors.sh" "$DARK_CONTRACTS"

if [[ "${PARITY_SKIP_REPLAY_TESTS:-0}" == "1" ]]; then
  echo "Skipping Rust, Go, and Node.js replay tests (PARITY_SKIP_REPLAY_TESTS=1)"
  exit 0
fi

echo "Replaying every row in Rust"
(cd "$REPO_ROOT" && cargo test -p lunarbase-pmm-math)

echo "Replaying every row in Go"
(cd "$REPO_ROOT/math/go" && go test ./...)

echo "Replaying every row in Node.js"
(cd "$REPO_ROOT/math/rust-node/lunarbase-pmm-math-node" && npm run build && npm test)
