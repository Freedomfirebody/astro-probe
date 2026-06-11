package com.example.complex.strategy;

import com.example.complex.model.Product;
import org.springframework.stereotype.Component;

import java.math.BigDecimal;
import java.math.RoundingMode;

@Component("bulkPricing")
public class BulkPricingStrategy implements PricingStrategy {

    private static final int BULK_THRESHOLD = 10;
    private static final BigDecimal BULK_DISCOUNT = new BigDecimal("0.10"); // 10% discount

    @Override
    public BigDecimal calculatePrice(Product product, int quantity) {
        BigDecimal baseTotal = product.getPrice().multiply(BigDecimal.valueOf(quantity));
        if (quantity > BULK_THRESHOLD) {
            BigDecimal discount = baseTotal.multiply(BULK_DISCOUNT);
            return baseTotal.subtract(discount).setScale(2, RoundingMode.HALF_UP);
        }
        return baseTotal;
    }

    @Override
    public String getStrategyName() {
        return "BULK";
    }
}
