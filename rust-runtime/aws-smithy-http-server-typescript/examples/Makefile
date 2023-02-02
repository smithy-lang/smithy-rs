SRC_DIR := $(shell git rev-parse --show-toplevel)
CUR_DIR := $(shell pwd)
GRADLE := $(SRC_DIR)/gradlew
SERVER_SDK_DST := $(CUR_DIR)/pokemon-service-server-sdk
CLIENT_SDK_DST := $(CUR_DIR)/pokemon-service-client
SERVER_SDK_SRC := $(SRC_DIR)/codegen-server-test/typescript/build/smithyprojections/codegen-server-test-typescript/pokemon-service-server-sdk/rust-server-codegen-typescript
CLIENT_SDK_SRC := $(SRC_DIR)/codegen-client-test/build/smithyprojections/codegen-client-test/pokemon-service-client/rust-client-codegen

all: codegen

codegen:
	$(GRADLE) --project-dir $(SRC_DIR) -P modules='pokemon-service-server-sdk,pokemon-service-client' :codegen-client-test:assemble :codegen-server-test:typescript:assemble
	mkdir -p $(SERVER_SDK_DST) $(CLIENT_SDK_DST)
	cp -av $(SERVER_SDK_SRC)/* $(SERVER_SDK_DST)/
	cp -av $(CLIENT_SDK_SRC)/* $(CLIENT_SDK_DST)/

build:
	npm run build:debug

release:
	npm run build

run: build
	ts-node app.ts

run-release: release
	ts-node app.ts

doc-open: codegen
	cargo doc --no-deps --open

clean:
	cargo clean || echo "Unable to run cargo clean"

distclean: clean
	rm -rf $(SERVER_SDK_DST) $(CLIENT_SDK_DST) $(CUR_DIR)/Cargo.lock

.PHONY: all
