package com.example.complex.strategy;

import com.example.complex.model.Product;
import org.springframework.stereotype.Component;

import java.math.BigDecimal;
import java.math.RoundingMode;

@Component("premiumPricing")
public class PremiumPricingStrategy implements PricingStrategy {

    private static final BigDecimal PREMIUM_MARKUP = new BigDecimal("0.15"); // 15% markup

    @Override
    public BigDecimal calculatePrice(Product product, int quantity) {
        BigDecimal baseTotal = product.getPrice().multiply(BigDecimal.valueOf(quantity));
        BigDecimal markup = baseTotal.multiply(PREMIUM_MARKUP);
        return baseTotal.add(markup).setScale(2, RoundingMode.HALF_UP);
    }

    @Override
    public String getStrategyName() {
        return "PREMIUM";
    }
}
