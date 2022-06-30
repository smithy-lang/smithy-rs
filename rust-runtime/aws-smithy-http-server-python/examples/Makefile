SRC_DIR := $(shell git rev-parse --show-toplevel)
CUR_DIR := $(shell pwd)
GRADLE := $(SRC_DIR)/gradlew
SERVER_SDK_DST := $(CUR_DIR)/pokemon_service_sdk
CLIENT_SDK_DST := $(CUR_DIR)/pokemon_service_client
SERVER_SDK_SRC := $(SRC_DIR)/codegen-server-test/python/build/smithyprojections/codegen-server-test-python/pokemon_service_sdk/rust-server-codegen-python
CLIENT_SDK_SRC := $(SRC_DIR)/codegen-test/build/smithyprojections/codegen-test/pokemon_service_client/rust-codegen
DEBUG_SHARED_LIBRARY_SRC := $(SRC_DIR)/target/debug/libpokemon_service_sdk.so
RELEASE_SHARED_LIBRARY_SRC := $(SRC_DIR)/target/release/libpokemon_service_sdk.so

all: codegen

codegen:
	$(GRADLE) --project-dir $(SRC_DIR) -P modules='pokemon_service_sdk,pokemon_service_client' :codegen-test:assemble :codegen-server-test:python:assemble
	mkdir -p $(SERVER_SDK_DST) $(CLIENT_SDK_DST)
	cp -av $(SERVER_SDK_SRC)/* $(SERVER_SDK_DST)/
	cp -av $(CLIENT_SDK_SRC)/* $(CLIENT_SDK_DST)/

build: codegen
	cargo build
	ln -sf $(DEBUG_SHARED_LIBRARY_SRC) $(CUR_DIR)/libpokemon_service_sdk.so

release: codegen
	cargo build --release
	ln -sf $(RELEASE_SHARED_LIBRARY_SRC) $(CUR_DIR)/libpokemon_service_sdk.so

run: build
	python $(CUR_DIR)/pokemon_service.py

test: build
	cargo test

doc-open: codegen
	cargo doc --no-deps --open

clean:
	cargo clean || echo "Unable to run cargo clean"

distclean: clean
	rm -rf $(SERVER_SDK_DST) $(CLIENT_SDK_DST) $(CUR_DIR)/Cargo.lock $(CUR_DIR)/libpokemon_service_sdk.so

.PHONY: all
