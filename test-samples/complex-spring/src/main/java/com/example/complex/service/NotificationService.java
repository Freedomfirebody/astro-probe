package com.example.complex.service;

import com.example.complex.model.Notification;

import java.util.List;

public interface NotificationService {

    Notification notifyUser(Long userId, String type, String message);

    void notifyOrderStatusChange(Long orderId, String oldStatus, String newStatus);

    void notifyStockDepleted(Long productId, String productName);

    List<Notification> getUnreadNotifications(Long userId);

    List<Notification> getAllNotifications(Long userId);

    void markAsRead(Long notificationId);

    void markAllAsRead(Long userId);

    long getUnreadCount(Long userId);
}
