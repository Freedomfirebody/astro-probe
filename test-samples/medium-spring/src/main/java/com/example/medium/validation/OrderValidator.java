package com.example.medium.validation;

import com.example.medium.dto.OrderDto;

import org.springframework.stereotype.Component;

import java.util.List;

@Component
public class OrderValidator {

    public ValidationResult validate(OrderDto orderDto) {
        ValidationResult result = new ValidationResult();

        if (orderDto == null) {
            result.addError("Order data must not be null");
            return result;
        }

        if (orderDto.getUserId() == null) {
            result.addError("User ID is required");
        }

        List<Long> productIds = orderDto.getProductIds();
        List<Integer> quantities = orderDto.getQuantities();

        if (productIds == null || productIds.isEmpty()) {
            result.addError("At least one product is required");
        }

        if (quantities == null || quantities.isEmpty()) {
            result.addError("Quantities must be provided");
        }

        if (productIds != null && quantities != null && productIds.size() != quantities.size()) {
            result.addError("Product IDs and quantities must have the same number of entries");
        }

        if (quantities != null) {
            for (int i = 0; i < quantities.size(); i++) {
                if (quantities.get(i) == null || quantities.get(i) <= 0) {
                    result.addError("Quantity at index " + i + " must be a positive integer");
                }
            }
        }

        return result;
    }
}
