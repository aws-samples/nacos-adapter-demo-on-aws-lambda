# perf-efs

This example does's use the nacos adapter. It just contains a simple javascript file to test the `stat` and `read` performance of Amazon EFS when accessing from AWS Lambda.

## Build

```sh
sam build
```

## Deploy

```sh
sam deploy -g
```

## Clean

```sh
sam delete
```
