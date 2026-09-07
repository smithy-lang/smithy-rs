SRC_DIR := $(shell git rev-parse --show-toplevel)
CUR_DIR := $(shell pwd)
GRADLE := $(SRC_DIR)/gradlew

SERVER_MODULES := pokemon-service-protocols-rest-json1-server-sdk,pokemon-service-protocols-rest-xml-server-sdk,pokemon-service-protocols-aws-json-10-server-sdk,pokemon-service-protocols-aws-json-11-server-sdk,pokemon-service-protocols-rpcv2-cbor-server-sdk

SERVER_SRC := $(SRC_DIR)/codegen-server-test/build/smithyprojections/codegen-server-test

all: codegen

codegen:
	$(GRADLE) --project-dir $(SRC_DIR) -P modules='$(SERVER_MODULES)' :codegen-server-test:assemble --quiet
	mkdir -p rest-json1/server-sdk
	mkdir -p rest-xml/server-sdk
	mkdir -p aws-json-10/server-sdk
	mkdir -p aws-json-11/server-sdk
	mkdir -p rpcv2-cbor/server-sdk
	cp -a $(SERVER_SRC)/pokemon-service-protocols-rest-json1-server-sdk/rust-server-codegen/. rest-json1/server-sdk/
	cp -a $(SERVER_SRC)/pokemon-service-protocols-rest-xml-server-sdk/rust-server-codegen/. rest-xml/server-sdk/
	cp -a $(SERVER_SRC)/pokemon-service-protocols-aws-json-10-server-sdk/rust-server-codegen/. aws-json-10/server-sdk/
	cp -a $(SERVER_SRC)/pokemon-service-protocols-aws-json-11-server-sdk/rust-server-codegen/. aws-json-11/server-sdk/
	cp -a $(SERVER_SRC)/pokemon-service-protocols-rpcv2-cbor-server-sdk/rust-server-codegen/. rpcv2-cbor/server-sdk/

check: codegen
	cargo check --all-targets --no-default-features --features rest-json1
	cargo check --all-targets --no-default-features --features rest-xml
	cargo check --all-targets --no-default-features --features aws-json-10
	cargo check --all-targets --no-default-features --features aws-json-11
	cargo check --all-targets --no-default-features --features rpcv2-cbor

clean:
	cargo clean || echo "Unable to run cargo clean"

.PHONY: all codegen check clean
