## Caveats

In AWS Lambda only `/tmp` is writable, so you have to configure `JM.LOG.PATH` and `JM.SNAPSHOT.PATH` to point to `/tmp` directory. This can be done using an environment variable in the Lambda function configuration: `JAVA_TOOL_OPTIONS: -DJM.LOG.PATH=/tmp/nacos/logs -DJM.SNAPSHOT.PATH=/tmp/`, see [`template.yml`](./template.yml).

## Build

Run these commands in the **_root_** folder of the project:

```sh
rm -rf ./examples/simple-java/layer/
mkdir -p ./examples/simple-java/layer/extensions

cargo build --release --target x86_64-unknown-linux-musl
cp ./target/x86_64-unknown-linux-musl/release/aws-lambda-nacos-adapter ./examples/simple-java/layer/extensions/

pushd ./examples/simple-java/
sam build -u
popd
```

## Deploy

Run these commands in the **_current_** folder:

```sh
sam deploy -g
```

## Clean

Run these commands in the **_current_** folder:

```sh
sam delete
```
