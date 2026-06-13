package com.example.complex.controller;

import com.example.complex.dto.OrderDto;
import com.example.complex.dto.OrderItemDto;
import com.example.complex.exception.ResourceNotFoundException;
import com.example.complex.mapper.EntityMapper;
import com.example.complex.model.Order;
import com.example.complex.service.OrderService;
import org.springframework.http.HttpStatus;
import org.springframework.http.ResponseEntity;
import org.springframework.web.bind.annotation.DeleteMapping;
import org.springframework.web.bind.annotation.GetMapping;
import org.springframework.web.bind.annotation.PathVariable;
import org.springframework.web.bind.annotation.PostMapping;
import org.springframework.web.bind.annotation.PutMapping;
import org.springframework.web.bind.annotation.RequestBody;
import org.springframework.web.bind.annotation.RequestMapping;
import org.springframework.web.bind.annotation.RequestParam;
import org.springframework.web.bind.annotation.RestController;

import java.util.List;
import java.util.Map;
import java.util.concurrent.CompletableFuture;
import java.util.stream.Collectors;

@RestController
@RequestMapping("/api/orders")
public class OrderController {

    private final OrderService orderService;
    private final EntityMapper entityMapper;

    public OrderController(OrderService orderService, EntityMapper entityMapper) {
        this.orderService = orderService;
        this.entityMapper = entityMapper;
    }

    @GetMapping
    public ResponseEntity<List<OrderDto>> getAllOrders() {
        List<OrderDto> orders = orderService.findAll().stream()
                .map(entityMapper::toOrderDto)
                .collect(Collectors.toList());
        return ResponseEntity.ok(orders);
    }

    @GetMapping("/{id}")
    public ResponseEntity<OrderDto> getOrderById(@PathVariable Long id) {
        return ResponseEntity.ok(orderService.getOrderDetails(id));
    }

    @GetMapping("/user/{userId}")
    public ResponseEntity<List<OrderDto>> getOrdersByUser(@PathVariable Long userId) {
        List<OrderDto> orders = orderService.findByUserId(userId).stream()
                .map(entityMapper::toOrderDto)
                .collect(Collectors.toList());
        return ResponseEntity.ok(orders);
    }

    @PostMapping
    public ResponseEntity<OrderDto> createOrder(@RequestParam Long userId, @RequestBody List<OrderItemDto> items) {
        Order order = orderService.createOrder(userId, items);
        return ResponseEntity.status(HttpStatus.CREATED).body(entityMapper.toOrderDto(order));
    }

    @PutMapping("/{id}/process")
    public ResponseEntity<Map<String, String>> processOrder(@PathVariable Long id) {
        // This triggers the async order processing pipeline
        CompletableFuture.runAsync(() -> orderService.processOrder(id));
        return ResponseEntity.accepted().body(
                Map.of("message", "Order processing started", "orderId", id.toString())
        );
    }

    @PutMapping("/{id}/cancel")
    public ResponseEntity<OrderDto> cancelOrder(@PathVariable Long id) {
        orderService.cancelOrder(id);
        Order order = orderService.findById(id)
                .orElseThrow(() -> new ResourceNotFoundException("Order", "id", id));
        return ResponseEntity.ok(entityMapper.toOrderDto(order));
    }
}
