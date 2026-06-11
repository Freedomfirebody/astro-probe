package com.example.complex.dto;

import java.math.BigDecimal;
import java.util.List;

public class OrderDto {

    private Long id;
    private Long userId;
    private String status;
    private BigDecimal totalAmount;
    private String orderNumber;
    private List<OrderItemDto> items;

    public OrderDto() {
    }

    public OrderDto(Long id, Long userId, String status, BigDecimal totalAmount, String orderNumber, List<OrderItemDto> items) {
        this.id = id;
        this.userId = userId;
        this.status = status;
        this.totalAmount = totalAmount;
        this.orderNumber = orderNumber;
        this.items = items;
    }

    public Long getId() {
        return id;
    }

    public void setId(Long id) {
        this.id = id;
    }

    public Long getUserId() {
        return userId;
    }

    public void setUserId(Long userId) {
        this.userId = userId;
    }

    public String getStatus() {
        return status;
    }

    public void setStatus(String status) {
        this.status = status;
    }

    public BigDecimal getTotalAmount() {
        return totalAmount;
    }

    public void setTotalAmount(BigDecimal totalAmount) {
        this.totalAmount = totalAmount;
    }

    public String getOrderNumber() {
        return orderNumber;
    }

    public void setOrderNumber(String orderNumber) {
        this.orderNumber = orderNumber;
    }

    public List<OrderItemDto> getItems() {
        return items;
    }

    public void setItems(List<OrderItemDto> items) {
        this.items = items;
    }
}
