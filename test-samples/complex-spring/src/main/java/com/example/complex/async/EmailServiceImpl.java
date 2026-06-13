package com.example.complex.async;

import com.example.complex.model.Order;
import com.example.complex.model.Product;
import org.slf4j.Logger;
import org.slf4j.LoggerFactory;
import org.springframework.scheduling.annotation.Async;
import org.springframework.stereotype.Service;

@Service
public class EmailServiceImpl implements EmailService {

    private static final Logger logger = LoggerFactory.getLogger(EmailServiceImpl.class);

    @Override
    @Async("taskExecutor")
    public void sendOrderConfirmation(Order order) {
        logger.info("Sending order confirmation email for order #{} to userId: {}",
                order.getOrderNumber(), order.getUserId());
        try {
            // Simulate email sending delay
            Thread.sleep(1000);
            logger.info("Order confirmation email sent successfully for order #{}", order.getOrderNumber());
        } catch (InterruptedException e) {
            Thread.currentThread().interrupt();
            logger.error("Failed to send order confirmation email for order #{}", order.getOrderNumber(), e);
        }
    }

    @Override
    @Async("taskExecutor")
    public void sendStockAlert(Product product) {
        logger.info("Sending stock alert email for product: {} (ID: {})", product.getName(), product.getId());
        try {
            // Simulate email sending delay
            Thread.sleep(500);
            logger.info("Stock alert email sent successfully for product: {}", product.getName());
        } catch (InterruptedException e) {
            Thread.currentThread().interrupt();
            logger.error("Failed to send stock alert email for product: {}", product.getName(), e);
        }
    }

    @Override
    @Async("taskExecutor")
    public void sendOrderCancellation(Order order) {
        logger.info("Sending order cancellation email for order #{} to userId: {}",
                order.getOrderNumber(), order.getUserId());
        try {
            // Simulate email sending delay
            Thread.sleep(800);
            logger.info("Order cancellation email sent successfully for order #{}", order.getOrderNumber());
        } catch (InterruptedException e) {
            Thread.currentThread().interrupt();
            logger.error("Failed to send order cancellation email for order #{}", order.getOrderNumber(), e);
        }
    }
}
