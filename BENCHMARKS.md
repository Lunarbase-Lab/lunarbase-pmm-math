# Benchmarks

These benchmarks cover the 0.4 punishment-era kernel. Results from the former
concentration curve are not comparable: quotes are now linear at the anchor,
and every triggering quote includes an immediate directional fee transition
that is persisted only after successful settlement.

## Run

```sh
make bench

# Individual packages
cargo bench -p lunarbase-pmm-math
cd math/go && go test -bench=. -benchmem -run=^$ -count=3
```

Criterion reports are written below `target/criterion/`. Go prints allocation
counts directly. Benchmarks are intentionally excluded from CI because shared
runners are too noisy for regression decisions.

## Covered paths

- base X -> Y and Y -> X nested-floor quotes;
- caller fee multiplier and full-fee sentinel paths;
- desired punishment with ceil rounding and the conceptual-Q24 sentinel;
- saturating directional fee transition;
- complete standard-token quote + punishment + reserve simulation;
- tiny, typical, near-reserve, and rejected amounts.

The Go `Into` APIs are expected to remain at `0 B/op, 0 allocs/op` for valid
preallocated inputs. Rust's hot quote and punishment paths allocate no heap
memory.

## Interpreting changes

Compare on the same host, toolchain, power profile, and command. Record at
least three runs and investigate:

- any new allocation on an `Into` path;
- a reproducible median regression above 10%;
- a changed result or branch mix in the benchmark fixture;
- a speed-up caused by removing width/error validation.

Correct bit-for-bit Solidity parity takes priority over benchmark movement.
Run `./scripts/regenerate-vectors.sh` before accepting any arithmetic
optimization.
