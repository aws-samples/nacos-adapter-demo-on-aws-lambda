## Caveats

In AWS Lambda only `/tmp` is writable, so you have to configure `JM.LOG.PATH` and `JM.SNAPSHOT.PATH` to point to `/tmp` directory. This can be done using an environment variable in the Lambda function configuration: `JAVA_TOOL_OPTIONS: -DJM.LOG.PATH=/tmp/nacos/logs -DJM.SNAPSHOT.PATH=/tmp/`, see [`template-passthrough.yml`](./template-passthrough.yml) or [`template-efs.yml`](./template-efs.yml).

## Build

### Passthrough Mode

```sh
rm -rf ./layer
mkdir -p ./layer/extensions

cargo build --release --target x86_64-unknown-linux-musl
cp ../../target/x86_64-unknown-linux-musl/release/aws-lambda-nacos-adapter ./layer/extensions/

cp ../../scripts/sync-entry-lwa.sh ./layer
chmod +x ./layer/sync-entry-lwa.sh

sam build -u -t template-passthrough.yml
```

### FS Mode

We use EFS to realize configuration updates.

```sh
rm -rf ./layer/
mkdir -p ./layer/extensions

cargo build --release --target x86_64-unknown-linux-musl
cp ../../target/x86_64-unknown-linux-musl/release/aws-lambda-nacos-adapter ./layer/extensions/

sam build -u -t template-efs.yml
```

## Deploy

```sh
sam deploy -g
```

## Clean

```sh
sam delete
```

## Credits

This example is based on the official [nacos-spring-boot-config-example](https://github.com/nacos-group/nacos-examples/tree/master/nacos-spring-boot-example/nacos-spring-boot-config-example).
