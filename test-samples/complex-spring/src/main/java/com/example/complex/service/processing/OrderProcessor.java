package com.example.complex.service.processing;

import com.example.complex.callback.ProcessingCallback;
import com.example.complex.exception.OrderProcessingException;
import com.example.complex.model.Order;
import com.example.complex.model.enums.OrderStatus;
import org.slf4j.Logger;
import org.slf4j.LoggerFactory;
import org.springframework.stereotype.Service;

@Service
public class OrderProcessor {

    private static final Logger logger = LoggerFactory.getLogger(OrderProcessor.class);

    public void process(Order order, ProcessingCallback<Order> callback) {
        logger.info("Starting order processing for order #{}", order.getOrderNumber());

        try {
            // Step 1: Validate order
            callback.onProgress(10);
            validateOrder(order);

            // Step 2: Reserve inventory
            callback.onProgress(30);
            reserveInventory(order);

            // Step 3: Process payment
            callback.onProgress(50);
            processPayment(order);

            // Step 4: Prepare shipment
            callback.onProgress(70);
            prepareShipment(order);

            // Step 5: Finalize
            callback.onProgress(90);
            order.setStatus(OrderStatus.PROCESSING);

            // Complete
            callback.onProgress(100);
            callback.onSuccess(order);

            logger.info("Order processing completed successfully for order #{}", order.getOrderNumber());
        } catch (Exception e) {
            logger.error("Order processing failed for order #{}: {}", order.getOrderNumber(), e.getMessage());
            callback.onFailure(e);
        }
    }

    private void validateOrder(Order order) {
        if (order.getItems() == null || order.getItems().isEmpty()) {
            throw new OrderProcessingException("Order has no items");
        }
        if (order.getTotalAmount() == null) {
            throw new OrderProcessingException("Order total amount is null");
        }
        logger.debug("Order #{} validated successfully", order.getOrderNumber());
    }

    private void reserveInventory(Order order) {
        // Simulate inventory reservation
        logger.debug("Inventory reserved for order #{}", order.getOrderNumber());
    }

    private void processPayment(Order order) {
        // Simulate payment processing
        logger.debug("Payment processed for order #{} - amount: {}", order.getOrderNumber(), order.getTotalAmount());
    }

    private void prepareShipment(Order order) {
        // Simulate shipment preparation
        logger.debug("Shipment prepared for order #{}", order.getOrderNumber());
    }
}
