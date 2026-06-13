package com.example.complex.service.impl;

import com.example.complex.async.EmailService;
import com.example.complex.callback.OrderProcessingCallback;
import com.example.complex.dto.OrderItemDto;
import com.example.complex.dto.OrderDto;
import com.example.complex.event.OrderCreatedEvent;
import com.example.complex.event.OrderStatusChangedEvent;
import com.example.complex.exception.OrderProcessingException;
import com.example.complex.exception.ResourceNotFoundException;
import com.example.complex.mapper.EntityMapper;
import com.example.complex.model.Order;
import com.example.complex.model.OrderItem;
import com.example.complex.model.Product;
import com.example.complex.model.enums.OrderStatus;
import com.example.complex.repository.OrderRepository;
import com.example.complex.service.OrderService;
import com.example.complex.service.ProductService;
import com.example.complex.service.UserService;
import com.example.complex.service.EventPublisherService;
import com.example.complex.service.processing.OrderProcessor;
import com.example.complex.strategy.PricingStrategy;
import com.example.complex.util.OrderNumberGenerator;
import org.slf4j.Logger;
import org.slf4j.LoggerFactory;
import org.springframework.beans.factory.annotation.Qualifier;
import org.springframework.stereotype.Service;
import org.springframework.transaction.annotation.Transactional;

import java.math.BigDecimal;
import java.util.ArrayList;
import java.util.List;
import java.util.Optional;

@Service
@Transactional
public class OrderServiceImpl implements OrderService {

    private static final Logger logger = LoggerFactory.getLogger(OrderServiceImpl.class);

    private final OrderRepository orderRepository;
    private final UserService userService;
    private final ProductService productService;
    private final EmailService emailService;
    private final EventPublisherService eventPublisherService;
    private final OrderProcessor orderProcessor;
    private final OrderProcessingCallback orderProcessingCallback;
    private final PricingStrategy pricingStrategy;
    private final OrderNumberGenerator orderNumberGenerator;
    private final EntityMapper entityMapper;

    public OrderServiceImpl(OrderRepository orderRepository,
                            UserService userService,
                            ProductService productService,
                            EmailService emailService,
                            EventPublisherService eventPublisherService,
                            OrderProcessor orderProcessor,
                            OrderProcessingCallback orderProcessingCallback,
                            @Qualifier("standardPricing") PricingStrategy pricingStrategy,
                            OrderNumberGenerator orderNumberGenerator,
                            EntityMapper entityMapper) {
        this.orderRepository = orderRepository;
        this.userService = userService;
        this.productService = productService;
        this.emailService = emailService;
        this.eventPublisherService = eventPublisherService;
        this.orderProcessor = orderProcessor;
        this.orderProcessingCallback = orderProcessingCallback;
        this.pricingStrategy = pricingStrategy;
        this.orderNumberGenerator = orderNumberGenerator;
        this.entityMapper = entityMapper;
    }

    @Override
    public Order createOrder(Long userId, List<OrderItemDto> itemDtos) {
        logger.info("Creating order for user: {}", userId);

        // Step 1: Verify user exists (cross-call to UserService)
        userService.findById(userId)
                .orElseThrow(() -> new ResourceNotFoundException("User", "id", userId));

        // Step 2: Create order shell
        Order order = new Order();
        order.setUserId(userId);
        order.setStatus(OrderStatus.PENDING);
        order.setOrderNumber(orderNumberGenerator.generate());
        order.setTotalAmount(BigDecimal.ZERO);

        BigDecimal totalAmount = BigDecimal.ZERO;
        List<OrderItem> orderItems = new ArrayList<>();

        // Step 3: Process each item - cross-call to ProductService for each
        for (OrderItemDto itemDto : itemDtos) {
            Product product = productService.findById(itemDto.getProductId())
                    .orElseThrow(() -> new ResourceNotFoundException("Product", "id", itemDto.getProductId()));

            // Step 4: Use PricingStrategy (injected via @Qualifier) to calculate prices
            BigDecimal itemPrice = pricingStrategy.calculatePrice(product, itemDto.getQuantity());

            OrderItem orderItem = new OrderItem();
            orderItem.setProductId(product.getId());
            orderItem.setQuantity(itemDto.getQuantity());
            orderItem.setUnitPrice(product.getPrice());

            orderItems.add(orderItem);
            totalAmount = totalAmount.add(itemPrice);

            // Step 5: Decrement stock (mutual call: OrderService → ProductService)
            productService.decrementStock(product.getId(), itemDto.getQuantity());
        }

        order.setTotalAmount(totalAmount);
        Order savedOrder = orderRepository.save(order);

        // Set orderId on items and add to order
        for (OrderItem item : orderItems) {
            item.setOrderId(savedOrder.getId());
            savedOrder.addItem(item);
        }
        savedOrder = orderRepository.save(savedOrder);

        // Step 6: Publish OrderCreatedEvent (event lineage)
        eventPublisherService.publishOrderCreated(savedOrder);

        // Step 7: Trigger async email (async lineage: order creation → email)
        emailService.sendOrderConfirmation(savedOrder);

        logger.info("Order created: {} (order number: {})", savedOrder.getId(), savedOrder.getOrderNumber());
        return savedOrder;
    }

    @Override
    public void processOrder(Long orderId) {
        logger.info("Processing order: {}", orderId);

        Order order = orderRepository.findById(orderId)
                .orElseThrow(() -> new ResourceNotFoundException("Order", "id", orderId));

        if (order.getStatus() != OrderStatus.PENDING && order.getStatus() != OrderStatus.CONFIRMED) {
            throw new OrderProcessingException("Order " + orderId + " cannot be processed in status: " + order.getStatus());
        }

        OrderStatus oldStatus = order.getStatus();
        order.setStatus(OrderStatus.PROCESSING);
        orderRepository.save(order);

        // Fire OrderStatusChangedEvent
        eventPublisherService.publishOrderStatusChanged(orderId, oldStatus, OrderStatus.PROCESSING);

        // Use OrderProcessor with callback pattern
        orderProcessor.process(order, orderProcessingCallback);
    }

    @Override
    public void cancelOrder(Long orderId) {
        logger.info("Cancelling order: {}", orderId);

        Order order = orderRepository.findById(orderId)
                .orElseThrow(() -> new ResourceNotFoundException("Order", "id", orderId));

        if (order.getStatus() == OrderStatus.DELIVERED || order.getStatus() == OrderStatus.CANCELLED) {
            throw new OrderProcessingException("Order " + orderId + " cannot be cancelled in status: " + order.getStatus());
        }

        OrderStatus oldStatus = order.getStatus();

        // Restore stock for each item (mutual call: OrderService → ProductService)
        for (OrderItem item : order.getItems()) {
            productService.restoreStock(item.getProductId(), item.getQuantity());
        }

        order.setStatus(OrderStatus.CANCELLED);
        orderRepository.save(order);

        // Fire OrderStatusChangedEvent
        eventPublisherService.publishOrderStatusChanged(orderId, oldStatus, OrderStatus.CANCELLED);

        // Trigger async cancellation email
        emailService.sendOrderCancellation(order);

        logger.info("Order cancelled: {}", orderId);
    }

    @Override
    @Transactional(readOnly = true)
    public Optional<Order> findById(Long id) {
        return orderRepository.findById(id);
    }

    @Override
    @Transactional(readOnly = true)
    public List<Order> findByUserId(Long userId) {
        return orderRepository.findByUserId(userId);
    }

    @Override
    @Transactional(readOnly = true)
    public List<Order> findAll() {
        return orderRepository.findAll();
    }

    @Override
    @Transactional(readOnly = true)
    public OrderDto getOrderDetails(Long orderId) {
        Order order = orderRepository.findById(orderId)
                .orElseThrow(() -> new ResourceNotFoundException("Order", "id", orderId));
        return entityMapper.toOrderDto(order);
    }
}
