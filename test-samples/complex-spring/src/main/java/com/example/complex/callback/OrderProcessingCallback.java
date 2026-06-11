package com.example.complex.callback;

import com.example.complex.model.Order;
import com.example.complex.model.enums.OrderStatus;
import com.example.complex.repository.OrderRepository;
import org.slf4j.Logger;
import org.slf4j.LoggerFactory;
import org.springframework.stereotype.Component;

@Component
public class OrderProcessingCallback implements ProcessingCallback<Order> {

    private static final Logger logger = LoggerFactory.getLogger(OrderProcessingCallback.class);

    private final OrderRepository orderRepository;

    public OrderProcessingCallback(OrderRepository orderRepository) {
        this.orderRepository = orderRepository;
    }

    @Override
    public void onSuccess(Order order) {
        logger.info("Order processing succeeded for order #{}", order.getOrderNumber());
        order.setStatus(OrderStatus.SHIPPED);
        orderRepository.save(order);
    }

    @Override
    public void onFailure(Exception e) {
        logger.error("Order processing failed: {}", e.getMessage(), e);
    }

    @Override
    public void onProgress(int percent) {
        logger.info("Order processing progress: {}%", percent);
    }
}
