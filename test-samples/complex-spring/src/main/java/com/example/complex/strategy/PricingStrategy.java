package com.example.complex.strategy;

import com.example.complex.model.Product;

import java.math.BigDecimal;

public interface PricingStrategy {

    BigDecimal calculatePrice(Product product, int quantity);

    String getStrategyName();
}
