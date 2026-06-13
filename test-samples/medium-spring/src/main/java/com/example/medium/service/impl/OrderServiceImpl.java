package com.example.medium.service.impl;

import com.example.medium.dto.OrderDto;
import com.example.medium.exception.ResourceNotFoundException;
import com.example.medium.model.Order;
import com.example.medium.model.Product;
import com.example.medium.model.User;
import com.example.medium.repository.OrderRepository;
import com.example.medium.service.OrderService;
import com.example.medium.service.ProductService;
import com.example.medium.service.UserService;
import com.example.medium.service.base.BaseService;
import com.example.medium.validation.OrderValidator;
import com.example.medium.validation.ValidationResult;

import org.springframework.beans.factory.annotation.Autowired;
import org.springframework.stereotype.Service;
import org.springframework.transaction.annotation.Transactional;

import java.math.BigDecimal;
import java.util.List;

@Service
@Transactional
public class OrderServiceImpl extends BaseService<Order, Long> implements OrderService {

    private final OrderRepository orderRepository;
    private final UserService userService;
    private final ProductService productService;
    private final OrderValidator orderValidator;

    @Autowired
    public OrderServiceImpl(OrderRepository orderRepository,
                            UserService userService,
                            ProductService productService,
                            OrderValidator orderValidator) {
        this.orderRepository = orderRepository;
        this.userService = userService;
        this.productService = productService;
        this.orderValidator = orderValidator;
    }

    @Override
    public Order createOrder(OrderDto orderDto) {
        // Validate order DTO
        ValidationResult validation = orderValidator.validate(orderDto);
        if (!validation.isValid()) {
            throw new IllegalArgumentException(
                    "Order validation failed: " + String.join(", ", validation.getErrors()));
        }

        // Validate user exists (cross-service call)
        User user = userService.findById(orderDto.getUserId());

        // Calculate total and validate products (cross-service call)
        BigDecimal totalAmount = BigDecimal.ZERO;
        List<Long> productIds = orderDto.getProductIds();
        List<Integer> quantities = orderDto.getQuantities();

        for (int i = 0; i < productIds.size(); i++) {
            Product product = productService.findById(productIds.get(i));
            int quantity = quantities.get(i);
            BigDecimal lineTotal = product.getPrice().multiply(BigDecimal.valueOf(quantity));
            totalAmount = totalAmount.add(lineTotal);

            // Decrement stock (cross-service call)
            productService.updateStock(product.getId(), -quantity);
        }

        // Create and save order
        Order order = new Order(user, totalAmount, "CREATED");
        Order savedOrder = orderRepository.save(order);
        logCreation("Order", savedOrder.getId());
        return savedOrder;
    }

    @Override
    @Transactional(readOnly = true)
    public Order findById(Long id) {
        logRetrieval("Order", id);
        return orderRepository.findById(id)
                .orElseThrow(() -> new ResourceNotFoundException("Order", "id", id));
    }

    @Override
    @Transactional(readOnly = true)
    public List<Order> findAll() {
        return orderRepository.findAll();
    }

    @Override
    @Transactional(readOnly = true)
    public List<Order> findByUserId(Long userId) {
        return orderRepository.findByUserId(userId);
    }

    @Override
    public void cancelOrder(Long id) {
        Order order = findById(id);

        if ("CANCELLED".equals(order.getStatus())) {
            throw new IllegalStateException("Order is already cancelled");
        }

        // Note: In a real app, we'd store order line items to reverse stock.
        // For this sample, we just change the status.
        order.setStatus("CANCELLED");
        orderRepository.save(order);
        logger.info("Order {} cancelled", id);
    }

    @Override
    public void deleteOrder(Long id) {
        Order order = findById(id);
        orderRepository.delete(order);
        logDeletion("Order", id);
    }
}
