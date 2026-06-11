package com.example.complex.event;

import org.springframework.context.ApplicationEvent;

public class StockDepletedEvent extends ApplicationEvent {

    private final Long productId;
    private final String productName;

    public StockDepletedEvent(Object source, Long productId, String productName) {
        super(source);
        this.productId = productId;
        this.productName = productName;
    }

    public Long getProductId() {
        return productId;
    }

    public String getProductName() {
        return productName;
    }
}
