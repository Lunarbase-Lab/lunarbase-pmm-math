# lunarbase-pmm-math — unified build/test entrypoint
#
# Layout:
#   rust/lunarbase-pmm-math            — pure Rust math (rlib, portable)
#   rust-node/lunarbase-pmm-math-node  — napi binding (cdylib, OS+arch specific)
#   go/                                — pure Go mirror (portable)
#
# Cross-compilation uses cargo-zigbuild (no Docker required).
# Setup once with:  make setup-cross
#
# Quick start:
#   make                            — build + test everything for the host
#   make ci                         — fmt-check + lint + test
#   make rust-cross TARGET=aarch64-unknown-linux-gnu
#   make node-cross-linux-x64
#   make go-cross-linux-amd64

GO          ?= go
CARGO       ?= cargo
ZIGBUILD    ?= $(CARGO) zigbuild
RUST_DIR    := math/rust
NODE_DIR    := math/rust-node/lunarbase-pmm-math-node
GO_DIR      := math/go

# Rust target triples for cross-compilation. Linux glibc version can be pinned
# by appending it to the triple, e.g. x86_64-unknown-linux-gnu.2.17 — useful
# for compatibility with older distros (RHEL 7, Ubuntu 18.04).
RUST_TARGETS_LINUX_X64   := x86_64-unknown-linux-gnu
RUST_TARGETS_LINUX_ARM64 := aarch64-unknown-linux-gnu
RUST_TARGETS_LINUX_MUSL  := x86_64-unknown-linux-musl
RUST_TARGETS_MAC_X64     := x86_64-apple-darwin
RUST_TARGETS_MAC_ARM64   := aarch64-apple-darwin
RUST_TARGETS_MAC_UNIV    := universal2-apple-darwin

ALL_RUST_CROSS_TARGETS := \
    $(RUST_TARGETS_LINUX_X64) \
    $(RUST_TARGETS_LINUX_ARM64) \
    $(RUST_TARGETS_LINUX_MUSL) \
    $(RUST_TARGETS_MAC_X64) \
    $(RUST_TARGETS_MAC_ARM64)

.PHONY: all ci setup-cross check-zig \
        rust-build rust-test rust-fmt rust-fmt-check rust-clippy rust-clean \
        rust-build-release rust-cross rust-bench \
        node-build node-test node-clean node-cross \
        node-cross-linux-x64 node-cross-linux-arm64 node-cross-linux-musl \
        node-cross-mac-x64 node-cross-mac-arm64 node-cross-mac-universal \
        node-cross-all \
        go-build go-test go-vet go-fmt go-fmt-check go-tidy go-clean \
        go-bench go-staticcheck \
        go-cross-linux-amd64 go-cross-linux-arm64 \
        go-cross-darwin-amd64 go-cross-darwin-arm64 \
        go-cross-windows-amd64 go-cross-all \
        publish-dry-run publish-crates-dry publish-npm-dry release-tag \
        build test clean fmt fmt-check lint bench bench-rust bench-go

# ---------- top-level ----------
all: build test

build: rust-build node-build go-build
test:  rust-test  node-test  go-test
clean: rust-clean node-clean go-clean
fmt:   rust-fmt   go-fmt
fmt-check: rust-fmt-check go-fmt-check
lint:  rust-clippy go-vet go-staticcheck

ci: fmt-check lint test

# ---------- rust core ----------
rust-build:
	$(CARGO) build -p lunarbase-pmm-math

rust-build-release:
	$(CARGO) build -p lunarbase-pmm-math --release

rust-test:
	$(CARGO) test -p lunarbase-pmm-math

rust-fmt:
	$(CARGO) fmt -p lunarbase-pmm-math

rust-fmt-check:
	$(CARGO) fmt -p lunarbase-pmm-math -- --check

rust-clippy:
	$(CARGO) clippy -p lunarbase-pmm-math --all-targets -- -D warnings

# Cross-compile rust core via cargo-zigbuild. Examples:
#   make rust-cross TARGET=aarch64-unknown-linux-gnu
#   make rust-cross TARGET=x86_64-unknown-linux-gnu.2.17   # pin glibc
TARGET ?=
rust-cross: check-zig
	@if [ -z "$(TARGET)" ]; then echo "usage: make rust-cross TARGET=<triple>"; exit 1; fi
	$(ZIGBUILD) -p lunarbase-pmm-math --release --target $(TARGET)

rust-clean:
	$(CARGO) clean -p lunarbase-pmm-math

# ---------- rust-node (napi binding, native) ----------
node-build:
	$(CARGO) build -p lunarbase-pmm-math-node --release

node-test:
	$(CARGO) test -p lunarbase-pmm-math-node

node-clean:
	$(CARGO) clean -p lunarbase-pmm-math-node

# Cross-compile the napi addon via cargo-zigbuild (no Docker needed).
# Output binaries:
#   linux:  target/<triple>/release/liblunarbase_pmm_math_node.so
#   darwin: target/<triple>/release/liblunarbase_pmm_math_node.dylib
# Rename the resulting library to `.node` before loading from Node.js.
# (Windows is not currently supported by cargo-zigbuild.)
node-cross: check-zig
	@if [ -z "$(TARGET)" ]; then echo "usage: make node-cross TARGET=<triple>"; exit 1; fi
	$(ZIGBUILD) -p lunarbase-pmm-math-node --release --target $(TARGET)

node-cross-linux-x64: check-zig
	$(ZIGBUILD) -p lunarbase-pmm-math-node --release --target $(RUST_TARGETS_LINUX_X64)

node-cross-linux-arm64: check-zig
	$(ZIGBUILD) -p lunarbase-pmm-math-node --release --target $(RUST_TARGETS_LINUX_ARM64)

node-cross-linux-musl: check-zig
	$(ZIGBUILD) -p lunarbase-pmm-math-node --release --target $(RUST_TARGETS_LINUX_MUSL)

node-cross-mac-x64: check-zig
	$(ZIGBUILD) -p lunarbase-pmm-math-node --release --target $(RUST_TARGETS_MAC_X64)

node-cross-mac-arm64: check-zig
	$(ZIGBUILD) -p lunarbase-pmm-math-node --release --target $(RUST_TARGETS_MAC_ARM64)

node-cross-mac-universal: check-zig
	$(ZIGBUILD) -p lunarbase-pmm-math-node --release --target $(RUST_TARGETS_MAC_UNIV)

# Build the addon for every supported (os, arch). Useful before publishing.
node-cross-all: \
        node-cross-linux-x64 \
        node-cross-linux-arm64 \
        node-cross-linux-musl \
        node-cross-mac-x64 \
        node-cross-mac-arm64

# ---------- go mirror ----------
go-build:
	cd $(GO_DIR) && $(GO) build ./...

go-test:
	cd $(GO_DIR) && $(GO) test ./...

go-vet:
	cd $(GO_DIR) && $(GO) vet ./...

# staticcheck — install with: go install honnef.co/go/tools/cmd/staticcheck@2024.1.1
go-staticcheck:
	cd $(GO_DIR) && staticcheck ./...

go-fmt:
	cd $(GO_DIR) && $(GO) fmt ./...

go-fmt-check:
	@cd $(GO_DIR) && diff=$$(gofmt -l .); \
	if [ -n "$$diff" ]; then echo "gofmt found unformatted files:"; echo "$$diff"; exit 1; fi

go-tidy:
	cd $(GO_DIR) && $(GO) mod tidy

go-clean:
	cd $(GO_DIR) && $(GO) clean -testcache

# ---------- benchmarks ----------
bench: bench-rust bench-go

bench-rust:
	$(CARGO) bench -p lunarbase-pmm-math

bench-go:
	cd $(GO_DIR) && $(GO) test -bench=. -benchmem -run=^$$ -count=3 ./...

# Go cross-compilation: pure Go, no CGO, works on any host.
go-cross-linux-amd64:
	cd $(GO_DIR) && CGO_ENABLED=0 GOOS=linux   GOARCH=amd64 $(GO) build ./...

go-cross-linux-arm64:
	cd $(GO_DIR) && CGO_ENABLED=0 GOOS=linux   GOARCH=arm64 $(GO) build ./...

go-cross-darwin-amd64:
	cd $(GO_DIR) && CGO_ENABLED=0 GOOS=darwin  GOARCH=amd64 $(GO) build ./...

go-cross-darwin-arm64:
	cd $(GO_DIR) && CGO_ENABLED=0 GOOS=darwin  GOARCH=arm64 $(GO) build ./...

go-cross-windows-amd64:
	cd $(GO_DIR) && CGO_ENABLED=0 GOOS=windows GOARCH=amd64 $(GO) build ./...

go-cross-all: \
        go-cross-linux-amd64 \
        go-cross-linux-arm64 \
        go-cross-darwin-amd64 \
        go-cross-darwin-arm64 \
        go-cross-windows-amd64

# ---------- publish (CI does the real publish; targets here are dry-run) ----------
# Real publishing happens via `.github/workflows/release.yml`, triggered by a
# `v*` tag push. Locally run `make publish-dry-run` before tagging to catch
# packaging errors early.

publish-crates-dry:
	$(CARGO) publish -p lunarbase-pmm-math --dry-run --allow-dirty

publish-npm-dry:
	cd $(NODE_DIR) && npm install --ignore-scripts
	cd $(NODE_DIR) && npm run build
	cd $(NODE_DIR) && npm pack --dry-run

publish-dry-run: publish-crates-dry publish-npm-dry
	@echo
	@echo "✓ crate packaging OK; npm package contents listed above."
	@echo "  Bump versions in:"
	@echo "    - Cargo.toml [workspace.package].version"
	@echo "    - $(NODE_DIR)/package.json .version + .optionalDependencies values"
	@echo "  Then: git tag vX.Y.Z && git push --tags"

# Convenience target that bumps the npm package version, prints the next steps,
# and exits without pushing anything. Use VERSION=X.Y.Z.
release-tag:
	@if [ -z "$(VERSION)" ]; then echo "usage: make release-tag VERSION=0.1.0"; exit 1; fi
	@echo "  →  reminder: bump Cargo.toml [workspace.package].version to $(VERSION)"
	@echo "  →  reminder: bump $(NODE_DIR)/package.json .version and .optionalDependencies"
	@echo "  →  then: git tag v$(VERSION) && git push origin v$(VERSION)"

# ---------- toolchain setup ----------
# Verify zig + cargo-zigbuild are available on PATH.
check-zig:
	@command -v zig >/dev/null 2>&1 || { echo "ERROR: zig not found. Install via 'brew install zig' or see https://ziglang.org/download/"; exit 1; }
	@$(CARGO) zigbuild --help >/dev/null 2>&1 || { echo "ERROR: cargo-zigbuild not installed. Run 'cargo install --locked cargo-zigbuild'"; exit 1; }

# Install zig + cargo-zigbuild + every supported rustup target.
# Run once per machine before using rust-cross / node-cross-*.
setup-cross:
	@command -v zig >/dev/null 2>&1 || { \
	    if command -v brew >/dev/null 2>&1; then brew install zig; \
	    else echo "Install zig manually: https://ziglang.org/download/"; exit 1; fi; }
	$(CARGO) install --locked cargo-zigbuild
	@for t in $(ALL_RUST_CROSS_TARGETS); do \
	    echo "rustup target add $$t"; \
	    rustup target add $$t; \
	done
