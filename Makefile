.PHONY: coverage coverage-baseline coverage-main coverage-clean

# Reproducible coverage workflow mirroring term-wm's Makefile
# (.github/workflows/rust-tests.yml in term-wm). Requires:
#   rustup component add llvm-tools-preview
#   cargo install cargo-llvm-cov
# Portable: relative paths only, POSIX sh, no platform-specific commands.

MAIN_WORKTREE := .build/main-worktree
COVERAGE_ARGS := --workspace --all-features
LCOV_OUT := lcov.info
BASELINE_OUT := coverage-baseline.txt

coverage:
	cargo llvm-cov clean --workspace
	cargo llvm-cov $(COVERAGE_ARGS) --lcov --output-path $(LCOV_OUT)
	cargo llvm-cov $(COVERAGE_ARGS) --summary-only

coverage-baseline:
	cargo llvm-cov clean --workspace
	cargo llvm-cov $(COVERAGE_ARGS) --lcov --output-path $(LCOV_OUT)
	@cargo llvm-cov $(COVERAGE_ARGS) --summary-only 2>&1 | tee $(BASELINE_OUT)

coverage-main:
	@mkdir -p .build
	@git worktree remove --force $(MAIN_WORKTREE) 2>/dev/null || true
	git worktree add $(MAIN_WORKTREE) main
	cd $(MAIN_WORKTREE) && cargo llvm-cov clean --workspace && cargo llvm-cov $(COVERAGE_ARGS) --lcov --output-path $(LCOV_OUT) && cargo llvm-cov $(COVERAGE_ARGS) --summary-only
	git worktree remove --force $(MAIN_WORKTREE)

coverage-clean:
	@git worktree remove --force $(MAIN_WORKTREE) 2>/dev/null || true
	rm -rf .build
	cargo llvm-cov clean --workspace
