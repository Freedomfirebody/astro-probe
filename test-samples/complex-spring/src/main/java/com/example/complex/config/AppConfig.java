package com.example.complex.config;

import com.example.complex.strategy.PricingStrategy;
import org.springframework.beans.factory.annotation.Qualifier;
import org.springframework.context.annotation.Bean;
import org.springframework.context.annotation.Configuration;

import java.util.HashMap;
import java.util.List;
import java.util.Map;

@Configuration
public class AppConfig {

    @Bean
    public Map<String, PricingStrategy> pricingStrategyMap(List<PricingStrategy> strategies) {
        Map<String, PricingStrategy> strategyMap = new HashMap<>();
        for (PricingStrategy strategy : strategies) {
            strategyMap.put(strategy.getStrategyName(), strategy);
        }
        return strategyMap;
    }

    @Bean("defaultPricingStrategy")
    public PricingStrategy defaultPricingStrategy(@Qualifier("standardPricing") PricingStrategy standardPricing) {
        return standardPricing;
    }
}
