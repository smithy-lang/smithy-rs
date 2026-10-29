SRC_DIR := $(shell git rev-parse --show-toplevel)
GRADLE := $(SRC_DIR)/gradlew
MODULES := multi-protocol-server-sdk,multi-protocol-client

all: run

# Generates one server and one client with runtime protocol selection. Regenerate the
# manifests and imported models so previous module filters cannot leave stale settings.
codegen:
	$(GRADLE) --project-dir $(SRC_DIR) -P modules='$(MODULES)' \
		:codegen-client-test:generateSmithyBuild --rerun :codegen-server-test:generateSmithyBuild --rerun \
		:codegen-client-test:generateCargoWorkspace --rerun :codegen-server-test:generateCargoWorkspace --rerun \
		:codegen-client-test:smithyBuild --rerun :codegen-server-test:smithyBuild --rerun \
		:codegen-client-test:assemble :codegen-server-test:assemble

run: codegen
	cargo run --quiet

.PHONY: all codegen run
