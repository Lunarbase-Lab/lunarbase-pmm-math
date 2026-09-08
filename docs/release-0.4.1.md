# 0.4.1 release notes

This source release adds protocol-neutral order-book APIs to Rust and Node.
The builders project LunarBase Pool state into directional ladders; target
systems integrate through separate publishing and settlement adapters. It is
prepared for publication; no registry publication or release tag is created here.

- Indicative sampled ladders expose `safety: indicative` and remain conservative
  relative to exact Pool quotes at cumulative sample sizes.
- Bounded exhaustive lot-policy ladders cover all allowed partial/sequential
  fills and both-direction interleavings from one immutable starting snapshot.
  Explicit budget exhaustion fails closed. The adapter enforces policy and
  guards exact output; unmodeled state changes remain outside the certificate.
- Fee-accounting preflight rejects missing partner operators for positive
  shares and insufficient uint112 treasury/global partner/per-router headroom.
  The certificate covers the finite price/reserve/punishment model, assuming
  fully credited fees and standard token behavior; arbitrary EVM call success
  remains outside that model. Partner share uses Pool's 1e6 denominator.
- Opt-in precise fitting emits up to 20 conservative levels under the same
  finite cursor/interleaving constraints. It reports achieved worst-state
  underquote, separate fresh-snapshot discount and `targetMet`, with independent
  graph and fitting work budgets. Existing flat/indicative APIs stay available.
- Sweep arithmetic uses the exact sum of floor-rounded consumed tranches,
  including lifetime cursor offsets. The removed VWAP round-trip could lose a
  raw output unit.
- Caller fee multiplier, maximum punishment, snapshot and maximum execution
  block are required for every builder. Paused/stale books contain no levels.
- Rust/Node package versions and generated metadata are aligned at 0.4.1.
  Existing Go quote/simulation APIs are unchanged; the orderbook APIs are
  currently Rust/Node only.

The input/output contract and integration requirements are documented in
[`order-book.md`](order-book.md). Protocol-specific wire formats, signatures
and execution semantics belong to the corresponding adapter implementations.
Pool arithmetic provenance remains in `solidity-parity/vector-manifest.json`.

Before registry publication, use the repository's existing release workflow to
build and test all supported native targets (macOS ARM64, Linux ARM64/x64 GNU,
Linux x64 musl), run `make publish-dry-run`, and create matching root/Go tags.
The local validation run cannot substitute for target-specific CI binaries.
