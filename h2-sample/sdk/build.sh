smithy build -c smithy-build-template.json
[ -d "rust-server-codegen" ] && rm -rf "rust-server-codegen"
mv build/smithy/server-client/rust-server-codegen .


