# Developer entry points. Every recipe runs in Docker (scripts/cargo-docker.sh or
# docker); nothing here needs a host Rust toolchain. See docs/DEVELOPMENT.md.
#
#   make help                     list targets
#   make dist                     ELF, AppImage, SHA256SUMS, notices and SBOM into dist/
#   CARGO_TARGET_DIR=target/agent-foo make test   per-agent build directory

SHELL := /bin/sh
.DEFAULT_GOAL := help

CARGO := scripts/cargo-docker.sh cargo
IMAGE ?= rust-microsip-toolchain:dev
DIST  ?= dist

.PHONY: help toolchain build release test fmt fmt-check clippy doc check verify dist \
	dist-check scan selftest context clean-clone lab-build lab-init suite clean shell

help: ## Show this help
	@awk 'BEGIN { FS = ":.*## " } /^[a-zA-Z_-]+:.*## / { printf "  %-12s %s\n", $$1, $$2 }' $(MAKEFILE_LIST)

toolchain: ## Build (or refresh) the pinned toolchain image
	docker buildx build --platform linux/amd64 --target toolchain --load -t $(IMAGE) .

build: ## Debug build of the workspace
	$(CARGO) build --workspace --locked

release: ## Release build (target/docker/release/rust-microsip; use `dist` for the published artifacts)
	$(CARGO) build --workspace --release --locked

test: ## Run all workspace tests (lab scenarios are separate: make suite)
	$(CARGO) test --workspace --locked

fmt: ## Format the workspace
	$(CARGO) fmt --all

fmt-check: ## Check formatting
	$(CARGO) fmt --all --check

clippy: ## Lint with clippy, warnings are errors
	$(CARGO) clippy --workspace --all-targets --locked -- -D warnings

doc: ## Rustdoc with warnings as errors
	RUSTDOCFLAGS='-D warnings' $(CARGO) doc --workspace --no-deps --locked

check: fmt-check clippy test doc ## Local checks: fmt-check, clippy, test (includes the layering rule and fuzzing), doc

verify: ## The CI verify stage: offline fmt, clippy, tests, rustdoc and licence notices in Docker
	docker buildx build --progress plain --platform linux/amd64 --target verify .

dist: ## Release artifacts (ELF, AppImage, SHA256SUMS, notices, SBOM) into DIST=dist
	docker buildx build --platform linux/amd64 --target artifacts --output type=local,dest=$(DIST) .

dist-check: ## Check the artifacts in DIST=dist (checksums, AppImage wraps the same ELF, publication scan)
	cd $(DIST) && sha256sum --check --strict SHA256SUMS
	sh packaging/appimage/verify.sh $(DIST)/rust-microsip-*-x86_64.AppImage $(DIST)/rust-microsip
	sh packaging/release/scan-publication.sh $(DIST)

scan: ## Publication scan of the index, history and staged content; run before every push
	sh packaging/release/scan-publication.sh

selftest: ## Prove the scanner and the release gate check block injected failures (no Docker)
	sh packaging/release/ci-selftest.sh --no-docker
	sh packaging/release/check-gates-selftest.sh

context: ## Docker context allowlist audit with a negative control
	sh packaging/check-context.sh

clean-clone: ## Build from a clean clone without base/ or host Rust (DIST-03)
	sh packaging/release/clean-clone-build.sh

lab-build: ## Build the disposable Asterisk lab image
	sh docker/asterisk/lab.sh build

lab-init: ## Generate the lab's fictional accounts and certificates
	sh docker/asterisk/lab.sh init

suite: ## Run the lab suite (all scenarios; SCENARIOS="gui-link gui-control" for a subset)
	sh docker/asterisk/suite.sh $(SCENARIOS)

clean: ## Delete target/ and DIST=dist (inside the checkout) without asking
	@case "$(DIST)" in /*|*..*|'') echo "DIST must be a relative path inside the checkout" >&2; exit 1 ;; esac
	rm -rf target $(DIST)

shell: ## Interactive shell in the toolchain container
	scripts/cargo-docker.sh bash
