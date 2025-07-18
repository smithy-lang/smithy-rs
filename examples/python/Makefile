SRC_DIR := $(shell git rev-parse --show-toplevel)
CUR_DIR := $(shell pwd)
GRADLE := $(SRC_DIR)/gradlew
WHEELS := $(CUR_DIR)/wheels
SERVER_SDK_DST := $(CUR_DIR)/pokemon-service-server-sdk
CLIENT_SDK_DST := $(CUR_DIR)/pokemon-service-client
SERVER_SDK_SRC := $(SRC_DIR)/codegen-server-test/python/build/smithyprojections/codegen-server-test-python/pokemon-service-server-sdk/rust-server-codegen-python
CLIENT_SDK_SRC := $(SRC_DIR)/codegen-client-test/build/smithyprojections/codegen-client-test/pokemon-service-client/rust-client-codegen

HAS_MATURIN := $(shell command -v maturin 2> /dev/null)

all: codegen

codegen:
	$(GRADLE) --project-dir $(SRC_DIR) -P modules='pokemon-service-server-sdk,pokemon-service-client' :codegen-client-test:assemble :codegen-server-test:codegen-server-test-python:assemble
	mkdir -p $(SERVER_SDK_DST) $(CLIENT_SDK_DST) $(WHEELS)
	cp -av $(SERVER_SDK_SRC)/* $(SERVER_SDK_DST)/
	cp -av $(CLIENT_SDK_SRC)/* $(CLIENT_SDK_DST)/

ensure-maturin:
ifndef HAS_MATURIN
    $(error "maturin is not available; please install it via 'pip install maturin' or 'cargo install maturin'")
endif

# Note on `--compatibility linux`: Maturin by default uses `manylinux_x_y` but it is not supported
# by our current CI version (3.7.10), we can drop `--compatibility linux` when we switch to higher Python version.
# For more detail: https://github.com/pypa/manylinux
build-wheel: ensure-maturin
	cd $(SERVER_SDK_DST) && maturin build --out $(WHEELS) --compatibility linux

build-wheel-release: ensure-maturin
	cd $(SERVER_SDK_DST) && maturin build --out $(WHEELS) --compatibility linux --release

install-wheel:
	find $(WHEELS) -type f -name '*.whl' | xargs python3 -m pip install --user --force-reinstall

generate-stubs:
	bash $(SERVER_SDK_DST)/stubgen.sh pokemon_service_server_sdk $(SERVER_SDK_DST)/Cargo.toml $(SERVER_SDK_DST)/python/pokemon_service_server_sdk

build: codegen
	$(MAKE) generate-stubs
	$(MAKE) build-wheel

release: codegen
	$(MAKE) generate-stubs
	$(MAKE) build-wheel-release

run: build install-wheel
	python3 $(CUR_DIR)/pokemon_service.py

run-release: release install-wheel
	python3 $(CUR_DIR)/pokemon_service.py

py-check: install-wheel
	python3 -m mypy pokemon_service.py

test: build py-check
	cargo test

clippy: codegen
	cargo clippy

doc-open: codegen
	cargo doc --no-deps --open

clean:
	cargo clean || echo "Unable to run cargo clean"

distclean: clean
	rm -rf $(SERVER_SDK_DST) $(SERVER_SDK_SRC) $(CLIENT_SDK_DST) $(CLIENT_SDK_SRC) $(WHEELS) $(CUR_DIR)/Cargo.lock

.PHONY: all
