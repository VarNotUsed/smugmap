.PHONY: build test clean install

INSTALL_DIR ?= /usr/local/lib

build:
	cargo build --release

test:
	cargo test
	@echo "Running integration selfcheck..."
	@cargo build 2>/dev/null
	@python3 test/serve.py 8787 & echo $$! > /tmp/smugmap-serve.pid; \
	sleep 0.3; \
	SMUGMAP_CONFIG=/tmp/smugmap-test.json \
		LD_PRELOAD=target/debug/libsmugmap.so \
		cargo run --bin selfcheck; \
	STATUS=$$?; \
	kill $$(cat /tmp/smugmap-serve.pid) 2>/dev/null; \
	exit $$STATUS

install:
	install -m 755 target/release/libsmugmap.so $(INSTALL_DIR)/smugmap.so

clean:
	cargo clean
