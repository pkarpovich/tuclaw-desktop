CARGO := mise exec -- cargo

.PHONY: build run test lint fmt fmt-check

build:
	$(CARGO) build --workspace --all-targets

run:
	$(CARGO) run -p tuclaw-desktop

test:
	$(CARGO) test --workspace

lint:
	$(CARGO) clippy --workspace --all-targets -- -D warnings

fmt:
	$(CARGO) fmt --all

fmt-check:
	$(CARGO) fmt --all -- --check
