SRC_DIR := $(shell git rev-parse --show-toplevel)
CUR_DIR := $(shell pwd)
GRADLE := $(SRC_DIR)/gradlew
SERVER_SDK_DST := $(CUR_DIR)/pokemon-service-sdk
CLIENT_SDK_DST := $(CUR_DIR)/pokemon-service-client
SERVER_SDK_SRC := $(SRC_DIR)/codegen-server-test/build/smithyprojections/codegen-server-test/pokemon-service-sdk/rust-server-codegen
CLIENT_SDK_SRC := $(SRC_DIR)/codegen-test/build/smithyprojections/codegen-test/pokemon-service-client/rust-codegen

all: build

codegen:
	# We need to cd to the top-level git dir otherwise gradle won't run.
	cd $(SRC_DIR); $(GRADLE) :codegen-test:assemble; $(GRADLE) :codegen-server-test:assemble
	mkdir	-p $(SERVER_SDK_DST) $(CLIENT_SDK_DST)
	cp -av $(SERVER_SDK_SRC)/* $(SERVER_SDK_DST)/
	cp -av $(CLIENT_SDK_SRC)/* $(CLIENT_SDK_DST)/

build: codegen
	cargo build

clean:
	cargo clean || echo "Unable to run cargo clean"

dist-clean: clean
	rm -rf $(SERVER_SDK_DST) $(CLIENT_SDK_DST) Cargo.lock

.PHONY: all
