package com.example.medium.service;

import com.example.medium.dto.OrderDto;
import com.example.medium.model.Order;

import java.util.List;

public interface OrderService {

    Order createOrder(OrderDto orderDto);

    Order findById(Long id);

    List<Order> findAll();

    List<Order> findByUserId(Long userId);

    void cancelOrder(Long id);

    void deleteOrder(Long id);
}
