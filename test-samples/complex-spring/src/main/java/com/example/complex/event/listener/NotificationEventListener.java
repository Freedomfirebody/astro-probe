package com.example.complex.event.listener;

import com.example.complex.event.OrderCreatedEvent;
import com.example.complex.event.OrderStatusChangedEvent;
import com.example.complex.event.StockDepletedEvent;
import com.example.complex.service.NotificationService;
import org.slf4j.Logger;
import org.slf4j.LoggerFactory;
import org.springframework.context.event.EventListener;
import org.springframework.stereotype.Component;

@Component
public class NotificationEventListener {

    private static final Logger logger = LoggerFactory.getLogger(NotificationEventListener.class);

    private final NotificationService notificationService;

    public NotificationEventListener(NotificationService notificationService) {
        this.notificationService = notificationService;
    }

    @EventListener
    public void handleOrderCreated(OrderCreatedEvent event) {
        logger.info("Handling OrderCreatedEvent for order: {}", event.getOrder().getId());
        notificationService.notifyUser(
                event.getOrder().getUserId(),
                "ORDER_CREATED",
                "Your order #" + event.getOrder().getOrderNumber() + " has been created successfully."
        );
    }

    @EventListener
    public void handleOrderStatusChanged(OrderStatusChangedEvent event) {
        logger.info("Handling OrderStatusChangedEvent for order {}: {} -> {}",
                event.getOrderId(), event.getOldStatus(), event.getNewStatus());
        // We need to find the order's userId - use a simple approach via notification service
        notificationService.notifyOrderStatusChange(
                event.getOrderId(),
                event.getOldStatus().name(),
                event.getNewStatus().name()
        );
    }

    @EventListener
    public void handleStockDepleted(StockDepletedEvent event) {
        logger.warn("Handling StockDepletedEvent for product {}: {}",
                event.getProductId(), event.getProductName());
        // Notify admin users about stock depletion
        notificationService.notifyStockDepleted(event.getProductId(), event.getProductName());
    }
}
