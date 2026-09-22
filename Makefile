.PHONY: build test test-linux docker-build clean clean-linux install

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

docker-build:
	docker build -t smugmap-dev -f Dockerfile.dev .

test-linux: docker-build
	docker run --rm \
		--privileged \
		-v "$(CURDIR):/smugmap" \
		-v smugmap-cargo-cache:/usr/local/cargo/registry \
		-v smugmap-target:/smugmap/target \
		-w /smugmap \
		smugmap-dev \
		bash -euc '\
		  cargo test; \
		  cargo build; \
		  printf "Hello, smugmap! 0123456789abcdef" > /tmp/smugmap-test-data.bin; \
		  printf "%s" "[{\"pattern\":\"*.bin\",\"url\":\"http://127.0.0.1:8787/tmp/smugmap-test-data.bin\"}]" > /tmp/smugmap-test.json; \
		  ( python3 test/serve.py 8787 & ); \
		  sleep 0.5; \
		  SMUGMAP_CONFIG=/tmp/smugmap-test.json LD_PRELOAD=target/debug/libsmugmap.so target/debug/selfcheck'

clean-linux:
	docker volume rm -f smugmap-cargo-cache smugmap-target

install:
	install -m 755 target/release/libsmugmap.so $(INSTALL_DIR)/smugmap.so

clean:
	cargo clean
