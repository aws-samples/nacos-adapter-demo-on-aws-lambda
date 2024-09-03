#!/bin/sh

exec java -cp "./:lib/*" "com.amazonaws.demo.config.NacosConfigApplication" "--server.port=${PORT}"