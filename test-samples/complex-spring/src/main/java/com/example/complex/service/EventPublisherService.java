package com.example.complex.service;

import com.example.complex.event.OrderCreatedEvent;
import com.example.complex.event.OrderStatusChangedEvent;
import com.example.complex.event.StockDepletedEvent;
import com.example.complex.model.Order;
import com.example.complex.model.enums.OrderStatus;
import org.slf4j.Logger;
import org.slf4j.LoggerFactory;
import org.springframework.context.ApplicationEventPublisher;
import org.springframework.stereotype.Service;

@Service
public class EventPublisherService {

    private static final Logger logger = LoggerFactory.getLogger(EventPublisherService.class);

    private final ApplicationEventPublisher applicationEventPublisher;

    public EventPublisherService(ApplicationEventPublisher applicationEventPublisher) {
        this.applicationEventPublisher = applicationEventPublisher;
    }

    public void publishOrderCreated(Order order) {
        logger.info("Publishing OrderCreatedEvent for order: {}", order.getId());
        OrderCreatedEvent event = new OrderCreatedEvent(this, order);
        applicationEventPublisher.publishEvent(event);
    }

    public void publishOrderStatusChanged(Long orderId, OrderStatus oldStatus, OrderStatus newStatus) {
        logger.info("Publishing OrderStatusChangedEvent for order {}: {} -> {}", orderId, oldStatus, newStatus);
        OrderStatusChangedEvent event = new OrderStatusChangedEvent(this, orderId, oldStatus, newStatus);
        applicationEventPublisher.publishEvent(event);
    }

    public void publishStockDepleted(Long productId, String productName) {
        logger.info("Publishing StockDepletedEvent for product {}: {}", productId, productName);
        StockDepletedEvent event = new StockDepletedEvent(this, productId, productName);
        applicationEventPublisher.publishEvent(event);
    }
}
