.PHONY: build test test-linux clean install

INSTALL_DIR ?= /usr/local/lib

build:
	cargo build --release

test:
	cargo test
	@echo "Running integration selfcheck..."
	@cargo build 2>/dev/null
	@SMUGMAP_CONFIG=/tmp/smugmap-test.json \
		LD_PRELOAD=target/debug/libsmugmap.so \
		cargo run --bin selfcheck

test-linux:
	docker run --rm \
		-v "$(CURDIR):/smugmap" \
		-w /smugmap \
		rust:latest \
		bash -c "cargo test && cargo build && SMUGMAP_CONFIG=/tmp/smugmap-test.json LD_PRELOAD=target/debug/libsmugmap.so cargo run --bin selfcheck"

install:
	install -m 755 target/release/libsmugmap.so $(INSTALL_DIR)/smugmap.so

clean:
	cargo clean
