package com.example.complex.strategy;

import com.example.complex.model.Product;
import org.springframework.stereotype.Component;

import java.math.BigDecimal;

@Component("standardPricing")
public class StandardPricingStrategy implements PricingStrategy {

    @Override
    public BigDecimal calculatePrice(Product product, int quantity) {
        return product.getPrice().multiply(BigDecimal.valueOf(quantity));
    }

    @Override
    public String getStrategyName() {
        return "STANDARD";
    }
}
