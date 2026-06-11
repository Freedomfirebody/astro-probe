package com.example.complex.service;

import com.example.complex.dto.OrderDto;
import com.example.complex.dto.OrderItemDto;
import com.example.complex.model.Order;

import java.util.List;
import java.util.Optional;

public interface OrderService {

    Order createOrder(Long userId, List<OrderItemDto> items);

    void processOrder(Long orderId);

    void cancelOrder(Long orderId);

    Optional<Order> findById(Long id);

    List<Order> findByUserId(Long userId);

    List<Order> findAll();

    OrderDto getOrderDetails(Long orderId);
}
