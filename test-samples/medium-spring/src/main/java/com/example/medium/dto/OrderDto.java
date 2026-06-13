package com.example.medium.dto;

import java.util.List;

public class OrderDto {

    private Long userId;
    private List<Long> productIds;
    private List<Integer> quantities;

    public OrderDto() {
    }

    public OrderDto(Long userId, List<Long> productIds, List<Integer> quantities) {
        this.userId = userId;
        this.productIds = productIds;
        this.quantities = quantities;
    }

    public Long getUserId() {
        return userId;
    }

    public void setUserId(Long userId) {
        this.userId = userId;
    }

    public List<Long> getProductIds() {
        return productIds;
    }

    public void setProductIds(List<Long> productIds) {
        this.productIds = productIds;
    }

    public List<Integer> getQuantities() {
        return quantities;
    }

    public void setQuantities(List<Integer> quantities) {
        this.quantities = quantities;
    }
}
