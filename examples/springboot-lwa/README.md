# Springboot with AWS Lambda Web Adapter

This example shows how to use this project with a springboot application and [AWS Lambda Web Adapter](https://github.com/awslabs/aws-lambda-web-adapter).

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

[ ! -f lwa.zip ] && aws lambda get-layer-version-by-arn --arn arn:aws:lambda:us-east-1:753240598075:layer:LambdaAdapterLayerX86:22 --output json | jq -r '.Content.Location' | xargs curl -o lwa.zip

unzip lwa.zip -d layer
mv ./layer/extensions/lambda-adapter ./layer
cp ../../scripts/lambda-adapter ./layer/extensions
chmod +x ./layer/extensions/lambda-adapter

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

[ ! -f lwa.zip ] && aws lambda get-layer-version-by-arn --arn arn:aws:lambda:us-east-1:753240598075:layer:LambdaAdapterLayerX86:22 --output json | jq -r '.Content.Location' | xargs curl -o lwa.zip

unzip lwa.zip -d layer
mv ./layer/extensions/lambda-adapter ./layer
cp ../../scripts/lambda-adapter ./layer/extensions
chmod +x ./layer/extensions/lambda-adapter

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

## Credits

This example is based on the official [nacos-spring-boot-config-example](https://github.com/nacos-group/nacos-examples/tree/master/nacos-spring-boot-example/nacos-spring-boot-config-example).
