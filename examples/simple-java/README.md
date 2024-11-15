# Simple Java

This example shows how to use this project with a simple java function.

## Caveats

In AWS Lambda only `/tmp` is writable, so you have to configure `JM.LOG.PATH` and `JM.SNAPSHOT.PATH` to point to `/tmp` directory. This can be done using an environment variable in the Lambda function configuration: `JAVA_TOOL_OPTIONS: -DJM.LOG.PATH=/tmp/nacos/logs -DJM.SNAPSHOT.PATH=/tmp/`.

## Build

### Passthrough Mode

Asynchronous update only:

```sh
rm -rf ./layer
mkdir -p ./layer/extensions

cargo build --release --target x86_64-unknown-linux-musl
cp ../../target/x86_64-unknown-linux-musl/release/aws-lambda-nacos-adapter ./layer/extensions/

sam build -u -t template-passthrough.yml
```

With synchronous update enabled:

```sh
rm -rf ./layer
mkdir -p ./layer/extensions

cargo build --release --target x86_64-unknown-linux-musl
cp ../../target/x86_64-unknown-linux-musl/release/aws-lambda-nacos-adapter ./layer/extensions/

cp ../../scripts/sync-entry.sh ./layer
chmod +x ./layer/sync-entry.sh

sam build -u -t template-passthrough-sync.yml
```

### FS Mode

We use Amazon EFS to realize configuration updates.

Asynchronous update only:

```sh
rm -rf ./layer/
mkdir -p ./layer/extensions

cargo build --release --target x86_64-unknown-linux-musl
cp ../../target/x86_64-unknown-linux-musl/release/aws-lambda-nacos-adapter ./layer/extensions/

sam build -u -t template-efs.yml
```

With synchronous update enabled:

```sh
rm -rf ./layer/
mkdir -p ./layer/extensions

cargo build --release --target x86_64-unknown-linux-musl
cp ../../target/x86_64-unknown-linux-musl/release/aws-lambda-nacos-adapter ./layer/extensions/

cp ../../scripts/sync-entry.sh ./layer
chmod +x ./layer/sync-entry.sh

sam build -u -t template-efs-sync.yml
```

## Deploy

```sh
sam deploy -g
```

## Clean

```sh
sam delete
```
