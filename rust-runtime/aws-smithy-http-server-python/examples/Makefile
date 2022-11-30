OS := $(shell uname -s)
SRC_DIR := $(shell git rev-parse --show-toplevel)
CUR_DIR := $(shell pwd)
GRADLE := $(SRC_DIR)/gradlew
SERVER_SDK_DST := $(CUR_DIR)/pokemon-service-server-sdk
CLIENT_SDK_DST := $(CUR_DIR)/pokemon-service-client
SERVER_SDK_SRC := $(SRC_DIR)/codegen-server-test/python/build/smithyprojections/codegen-server-test-python/pokemon-service-server-sdk/rust-server-codegen-python
CLIENT_SDK_SRC := $(SRC_DIR)/codegen-client-test/build/smithyprojections/codegen-client-test/pokemon-service-client/rust-codegen

SHARED_LIBRARY_DST := $(CUR_DIR)/libpokemon_service_server_sdk.so
ifeq ($(OS), Darwin)
	DEBUG_SHARED_LIBRARY_SRC := $(SRC_DIR)/target/debug/libpokemon_service_server_sdk.dylib
	RELEASE_SHARED_LIBRARY_SRC := $(SRC_DIR)/target/release/libpokemon_service_server_sdk.dylib
else
	DEBUG_SHARED_LIBRARY_SRC := $(SRC_DIR)/target/debug/libpokemon_service_server_sdk.so
	RELEASE_SHARED_LIBRARY_SRC := $(SRC_DIR)/target/release/libpokemon_service_server_sdk.so
endif

all: codegen

codegen:
	$(GRADLE) --project-dir $(SRC_DIR) -P modules='pokemon-service-server-sdk,pokemon-service-client' :codegen-client-test:assemble :codegen-server-test:python:assemble
	mkdir -p $(SERVER_SDK_DST) $(CLIENT_SDK_DST)
	cp -av $(SERVER_SDK_SRC)/* $(SERVER_SDK_DST)/
	cp -av $(CLIENT_SDK_SRC)/* $(CLIENT_SDK_DST)/

clippy: codegen
	cargo clippy

build: codegen
	cargo build
	ln -sf $(DEBUG_SHARED_LIBRARY_SRC) $(SHARED_LIBRARY_DST)

py_check: build
	mypy pokemon_service.py

release: codegen
	cargo build --release
	ln -sf $(RELEASE_SHARED_LIBRARY_SRC) $(SHARED_LIBRARY_DST)

run: build
	python $(CUR_DIR)/pokemon_service.py

test: build
	cargo test

doc-open: codegen
	cargo doc --no-deps --open

clean:
	cargo clean || echo "Unable to run cargo clean"

distclean: clean
	rm -rf $(SERVER_SDK_DST) $(CLIENT_SDK_DST) $(CUR_DIR)/Cargo.lock $(SHARED_LIBRARY_DST)

.PHONY: all
