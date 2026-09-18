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
		-v "$(CURDIR):/smugmap" \
		-v smugmap-cargo-cache:/usr/local/cargo/registry \
		-v smugmap-target:/smugmap/target \
		-w /smugmap \
		smugmap-dev \
		bash -c "cargo test && cargo build && SMUGMAP_CONFIG=/tmp/smugmap-test.json LD_PRELOAD=target/debug/libsmugmap.so cargo run --bin selfcheck"

clean-linux:
	docker volume rm -f smugmap-cargo-cache smugmap-target

install:
	install -m 755 target/release/libsmugmap.so $(INSTALL_DIR)/smugmap.so

clean:
	cargo clean
