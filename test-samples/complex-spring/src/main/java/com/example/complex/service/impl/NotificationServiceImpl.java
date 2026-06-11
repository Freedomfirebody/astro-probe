package com.example.complex.service.impl;

import com.example.complex.exception.ResourceNotFoundException;
import com.example.complex.model.Notification;
import com.example.complex.model.Order;
import com.example.complex.repository.NotificationRepository;
import com.example.complex.repository.OrderRepository;
import com.example.complex.repository.UserRepository;
import com.example.complex.service.NotificationService;
import org.slf4j.Logger;
import org.slf4j.LoggerFactory;
import org.springframework.stereotype.Service;
import org.springframework.transaction.annotation.Transactional;

import java.util.List;

@Service
@Transactional
public class NotificationServiceImpl implements NotificationService {

    private static final Logger logger = LoggerFactory.getLogger(NotificationServiceImpl.class);

    private final NotificationRepository notificationRepository;
    private final OrderRepository orderRepository;
    private final UserRepository userRepository;

    public NotificationServiceImpl(NotificationRepository notificationRepository,
                                   OrderRepository orderRepository,
                                   UserRepository userRepository) {
        this.notificationRepository = notificationRepository;
        this.orderRepository = orderRepository;
        this.userRepository = userRepository;
    }

    @Override
    public Notification notifyUser(Long userId, String type, String message) {
        logger.info("Creating notification for user {}: type={}, message={}", userId, type, message);
        Notification notification = new Notification(userId, type, message);
        return notificationRepository.save(notification);
    }

    @Override
    public void notifyOrderStatusChange(Long orderId, String oldStatus, String newStatus) {
        logger.info("Notifying order status change for order {}: {} -> {}", orderId, oldStatus, newStatus);
        Order order = orderRepository.findById(orderId)
                .orElseThrow(() -> new ResourceNotFoundException("Order", "id", orderId));

        String message = String.format("Your order #%s status changed from %s to %s",
                order.getOrderNumber(), oldStatus, newStatus);
        notifyUser(order.getUserId(), "ORDER_STATUS_CHANGED", message);
    }

    @Override
    public void notifyStockDepleted(Long productId, String productName) {
        logger.warn("Notifying stock depletion for product {}: {}", productId, productName);
        // Notify all admin users
        List<com.example.complex.model.User> admins = userRepository.findByRole("ADMIN");
        for (com.example.complex.model.User admin : admins) {
            String message = String.format("Product '%s' (ID: %d) is now out of stock!", productName, productId);
            notifyUser(admin.getId(), "STOCK_DEPLETED", message);
        }
    }

    @Override
    @Transactional(readOnly = true)
    public List<Notification> getUnreadNotifications(Long userId) {
        return notificationRepository.findByUserIdAndReadFalse(userId);
    }

    @Override
    @Transactional(readOnly = true)
    public List<Notification> getAllNotifications(Long userId) {
        return notificationRepository.findByUserId(userId);
    }

    @Override
    public void markAsRead(Long notificationId) {
        Notification notification = notificationRepository.findById(notificationId)
                .orElseThrow(() -> new ResourceNotFoundException("Notification", "id", notificationId));
        notification.setRead(true);
        notificationRepository.save(notification);
        logger.info("Notification {} marked as read", notificationId);
    }

    @Override
    public void markAllAsRead(Long userId) {
        int updated = notificationRepository.markAllAsRead(userId);
        logger.info("Marked {} notifications as read for user {}", updated, userId);
    }

    @Override
    @Transactional(readOnly = true)
    public long getUnreadCount(Long userId) {
        return notificationRepository.countByUserIdAndReadFalse(userId);
    }
}
