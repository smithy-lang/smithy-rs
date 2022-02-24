SERVER_SDK := pokemon-service-sdk
CLIENT_SDK := pokemon-service-client

codegen:
	mkdir -p $(SERVER_SDK)/src $(CLIENT_SDK)/src
	touch $(SERVER_SDK)/src/lib.rs $(CLIENT_SDK)/src/lib.rs
	sed 's#^name = "example"#name = "pokemon-service-sdk"#' .Cargo.toml.deps.tmpl > $(SERVER_SDK)/Cargo.toml
	sed 's#^name = "example"#name = "pokemon-service-client"#' .Cargo.toml.deps.tmpl > $(CLIENT_SDK)/Cargo.toml

build: codegen
	cargo build

release: codegen
	cargo build --release

clean:
	cargo clean

dist-clean:
	rm -rf $(SERVER_SDK) $(CLIENT_SDK)

default: build

.PHONY: default codegen build release clean dist-clean
