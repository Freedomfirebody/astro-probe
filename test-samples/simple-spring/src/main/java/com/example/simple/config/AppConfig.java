package com.example.simple.config;

import org.springframework.context.annotation.Bean;
import org.springframework.context.annotation.Configuration;

@Configuration
public class AppConfig {

    @Bean(name = "appName")
    public String appName() {
        return "simple-spring";
    }
}
