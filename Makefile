build-NacosAdapterLayer:
	cargo build --release --target x86_64-unknown-linux-musl
	mkdir -p $(ARTIFACTS_DIR)/extensions
	cp ./target/x86_64-unknown-linux-musl/release/aws-lambda-nacos-adapter $(ARTIFACTS_DIR)/extensions/aws-lambda-nacos-adapter
	cp ./scripts/sync-entry.sh $(ARTIFACTS_DIR)/