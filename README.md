# AWS Lambda Nacos Adapter

Let your AWS Lambda functions listen to your configuration changes on Nacos. Supports both Nacos v1 (HTTP) and Nacos v2 (gRPC).

## Usage

This can be used as an AWS Lambda Layer.

1. Run `sam build` to build the layer, then run `sam deploy` to deploy the layer.
2. Add the layer to your lambda function.
3. Set your function's [environment variables](#environment-variables) below to configure the adapter.
4. Specify the Nacos server to `127.0.0.1:8848` (or the port you set) for your app.

## Getting Started

### Choose a Config Provider

#### Passthrough Mode

In this mode, you must provide a valid Nacos server address via the `AWS_LAMBDA_NACOS_ADAPTER_ORIGIN_ADDRESS` environment variable. If your Nacos server can only be accessed via an AWS VPC using a private IP address, you have to [connect your AWS Lambda functions to the VPC](https://docs.aws.amazon.com/lambda/latest/dg/configuration-vpc.html) too.

When your AWS Lambda functions are invoked, the adapter will fetch the latest configuration from the Nacos server and notify your functions if the config changes.

#### FS Mode

In this mode, you can provide a path as the configuration source via the `AWS_LAMBDA_NACOS_ADAPTER_CONFIG_PATH` environment variable. The adapter will try to read `{AWS_LAMBDA_NACOS_ADAPTER_CONFIG_PATH}{tenant}/{group}/{dataId}` as the configuration.

If your configuration won't change, you can provide a static configuration file to the adapter. If your configuration will change, you can use a shared file system like Amazon EFS to share the configuration between your functions.

When your AWS Lambda functions are invoked, the adapter will read the latest configuration from the file system and notify your functions if the config changes.

### Enable Synchronous Update

By default, the adapter will update the configuration asynchronously, no matter the mode is passthrough or fs. The good thing about asynchronous update is that it won't introduce additional latency to your function's invocation. The downside is that the configuration update might be applied in the next invocation instead of the current one.

This should be fine if your function is invoked frequently. But if your function is invoked infrequently, you may want to update the configuration synchronously if the last update is too long ago, which means the configuration is updated first, then your handler function is invoked.

This might introduce additional latency to your function's invocation, you can control the cooldown time via the `AWS_LAMBDA_NACOS_ADAPTER_SYNC_COOLDOWN_MS` environment variables to prevent synchronous update from happening too frequently.

This feature requires runtime api proxy to realize, so you have to modify your handler function's environment variable `AWS_LAMBDA_RUNTIME_API` to override the default value. This environment variable is preserved by AWS Lambda, so you have to use [wrapper scripts](https://docs.aws.amazon.com/lambda/latest/dg/runtimes-modify.html) to override it. You can checkout the [`scripts/`](./scripts/) folder for the basic wrapper scripts.

## Environment Variables

### Config Provider

- `AWS_LAMBDA_NACOS_ADAPTER_ORIGIN_ADDRESS`
  - The address of the origin Nacos server.
  - Set this to enable the adapter to run in [passthrough mode](#passthrough-mode).
  - Example: `172.31.0.123:8848`.
- `AWS_LAMBDA_NACOS_ADAPTER_CONFIG_PATH`
  - The path to your configuration files.
  - Set this to enable the adapter to run in [fs mode](#fs-mode).
  - If you already set `AWS_LAMBDA_NACOS_ADAPTER_ORIGIN_ADDRESS`, this will be ignored.
  - Default: `/mnt/efs/nacos/`

### Asynchronous Update

- `AWS_LAMBDA_NACOS_ADAPTER_COOLDOWN_MS`
  - The cooldown in milliseconds before the adapter refresh the configuration again.
  - If you want your configuration to be applied as soon as possible, reduce this value. If you want to reduce the number of requests to the origin server, increase this value.
  - Default: `0`.

### Synchronous Update

- `AWS_LAMBDA_EXEC_WRAPPER`
  - Set this to `/opt/sync-entry.sh` (or `/opt/sync-entry-lwa.sh` if you are using [AWS Lambda Web Adapter](https://github.com/awslabs/aws-lambda-web-adapter)) to enable synchronous update.
  - See the [AWS official documentation](https://docs.aws.amazon.com/lambda/latest/dg/runtimes-modify.html) about this environment variable.
- `AWS_LAMBDA_NACOS_ADAPTER_SYNC_PORT`
  - The port number that the runtime API proxy listens on for synchronous update.
  - You can set this to any port number that is not used by other processes.
- `AWS_LAMBDA_NACOS_ADAPTER_SYNC_COOLDOWN_MS`
  - The cooldown in milliseconds before the adapter refresh the configuration again for synchronous update.
  - This should be no less than `AWS_LAMBDA_NACOS_ADAPTER_COOLDOWN_MS`.
  - Default: `0`.

### Misc

- `AWS_LAMBDA_NACOS_ADAPTER_PORT`
  - The port number that the mock Nacos listens on.
  - Default: `8848`.
- `AWS_LAMBDA_NACOS_ADAPTER_CACHE_SIZE`
  - The maximum number of entries that the cache can hold.
  - Default: `64`.

## [Examples](./examples/)

## Performance vs Timeliness

Here are examples of environment variable settings for different scenarios:

### Most Timely

By enabling synchronous update and set the cooldown to 0ms, the adapter will always update the configuration first, then invoke your handler function. This will make sure your handler function always gets the latest configuration.

```yaml
AWS_LAMBDA_NACOS_ADAPTER_SYNC_COOLDOWN_MS: 0
```

Performance: if the config is not changed, the adapter will introduce an additional delay to your function's **_each_** invocation when fetching the latest config; if the config is changed, besides the delay caused by fetching the config, the adapter will introduce an additional delay to wait for your configuration to be applied.

### Balanced

It's recommended to enable synchronous update, and set the cooldown to a reasonable value for both synchronous and asynchronous update.

```yaml
# all these values are adjustable
AWS_LAMBDA_NACOS_ADAPTER_COOLDOWN_MS: 5000 # 5 seconds
AWS_LAMBDA_NACOS_ADAPTER_SYNC_COOLDOWN_MS: 60000 # 1 minute
```

In the examples above, since the cooldown for synchronous update is much longer than the cooldown for asynchronous update, the adapter will update the configuration asynchronously for most invocations (if the function is frequently invoked), and update the configuration synchronously if the last update is too long ago.

## How It Works

```mermaid
sequenceDiagram
  participant L as AWS Lambda Service
  participant C as Nacos Client
  participant A as Nacos Adapter
  participant S as Config Provider
  box In the same AWS Lambda sandbox
    participant C
    participant A
  end
  A->>A: Start mock nacos server at localhost with AWS_LAMBDA_NACOS_ADAPTER_PORT
  A->>L: Register as AWS Lambda external extension
  C->>A: Get the initial configuration
  A->>S: Get the initial configuration
  A->>C: Return the initial configuration
  C->>A: Establish long connection, listen to config changes
  loop AWS Lambda Invocation
    par Synchronous Update (if enabled)
      L->>A: Next invocation (via runtime API proxy)
      A-->>S: Get latest config <br/> (if AWS_LAMBDA_NACOS_ADAPTER_SYNC_COOLDOWN_MS is reached)
      A-->>C: Notify the config change (if the config is changed)
      A-->>A: Wait for the configuration to be applied <br/> (if the config is changed)
      A->>C: Invoke the handler function
    and Asynchronous update
      L->>A: Next invocation (via AWS Lambda's Extension API)
      A-->>S: Get latest config <br/> (if AWS_LAMBDA_NACOS_ADAPTER_COOLDOWN_MS is reached <br/> and not updated by synchronous update)
      A-->>C: Notify the config change (if the config is changed)
      A-->>A: Wait for the configuration to be applied <br/> (if the config is changed)
      A->>L: End of this invocation (via AWS Lambda's Extension API)
    end
  end
```

## Credits

This project references many code snippets from [nacos-group/r-nacos](https://github.com/nacos-group/r-nacos/).

## [CHANGELOG](./CHANGELOG.md)
