package com.amazonaws.demo.config;

import com.alibaba.nacos.spring.context.annotation.config.NacosPropertySource;
import org.springframework.boot.SpringApplication;
import org.springframework.boot.autoconfigure.SpringBootApplication;
import org.springframework.web.bind.annotation.RequestMapping;
import org.springframework.web.bind.annotation.RequestMethod;

@SpringBootApplication
@NacosPropertySource(dataId = "example", autoRefreshed = true)
public class NacosConfigApplication {

    @RequestMapping(path = "/healthz", method = RequestMethod.GET)
    public String healthCheck() {
        return "healthy";
    }

    public static void main(String[] args) {
        SpringApplication.run(NacosConfigApplication.class, args);
    }
}