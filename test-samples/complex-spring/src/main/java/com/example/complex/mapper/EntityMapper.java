package com.example.complex.mapper;

import com.example.complex.dto.NotificationDto;
import com.example.complex.dto.OrderDto;
import com.example.complex.dto.OrderItemDto;
import com.example.complex.dto.ProductDto;
import com.example.complex.dto.UserDto;
import com.example.complex.model.Notification;
import com.example.complex.model.Order;
import com.example.complex.model.OrderItem;
import com.example.complex.model.Product;
import com.example.complex.model.User;
import org.springframework.stereotype.Component;

import java.util.List;
import java.util.stream.Collectors;

@Component
public class EntityMapper {

    // ---- User mappings ----

    public UserDto toUserDto(User user) {
        if (user == null) {
            return null;
        }
        return new UserDto(user.getId(), user.getName(), user.getEmail(), user.getRole());
    }

    public User toUserEntity(UserDto dto) {
        if (dto == null) {
            return null;
        }
        User user = new User(dto.getName(), dto.getEmail(), dto.getRole());
        user.setId(dto.getId());
        return user;
    }

    // ---- Product mappings ----

    public ProductDto toProductDto(Product product) {
        if (product == null) {
            return null;
        }
        return new ProductDto(
                product.getId(),
                product.getName(),
                product.getPrice(),
                product.getStock(),
                product.getStatus() != null ? product.getStatus().name() : null
        );
    }

    public Product toProductEntity(ProductDto dto) {
        if (dto == null) {
            return null;
        }
        Product product = new Product();
        product.setId(dto.getId());
        product.setName(dto.getName());
        product.setPrice(dto.getPrice());
        product.setStock(dto.getStock());
        return product;
    }

    // ---- Order mappings ----

    public OrderDto toOrderDto(Order order) {
        if (order == null) {
            return null;
        }
        List<OrderItemDto> itemDtos = null;
        if (order.getItems() != null) {
            itemDtos = order.getItems().stream()
                    .map(this::toOrderItemDto)
                    .collect(Collectors.toList());
        }
        return new OrderDto(
                order.getId(),
                order.getUserId(),
                order.getStatus() != null ? order.getStatus().name() : null,
                order.getTotalAmount(),
                order.getOrderNumber(),
                itemDtos
        );
    }

    public Order toOrderEntity(OrderDto dto) {
        if (dto == null) {
            return null;
        }
        Order order = new Order();
        order.setId(dto.getId());
        order.setUserId(dto.getUserId());
        order.setTotalAmount(dto.getTotalAmount());
        order.setOrderNumber(dto.getOrderNumber());
        return order;
    }

    // ---- OrderItem mappings ----

    public OrderItemDto toOrderItemDto(OrderItem item) {
        if (item == null) {
            return null;
        }
        return new OrderItemDto(item.getId(), item.getProductId(), item.getQuantity(), item.getUnitPrice());
    }

    public OrderItem toOrderItemEntity(OrderItemDto dto) {
        if (dto == null) {
            return null;
        }
        OrderItem item = new OrderItem();
        item.setId(dto.getId());
        item.setProductId(dto.getProductId());
        item.setQuantity(dto.getQuantity());
        item.setUnitPrice(dto.getUnitPrice());
        return item;
    }

    // ---- Notification mappings ----

    public NotificationDto toNotificationDto(Notification notification) {
        if (notification == null) {
            return null;
        }
        return new NotificationDto(
                notification.getId(),
                notification.getUserId(),
                notification.getType(),
                notification.getMessage(),
                notification.getSentAt(),
                notification.isRead()
        );
    }

    public Notification toNotificationEntity(NotificationDto dto) {
        if (dto == null) {
            return null;
        }
        Notification notification = new Notification(dto.getUserId(), dto.getType(), dto.getMessage());
        notification.setId(dto.getId());
        notification.setSentAt(dto.getSentAt());
        notification.setRead(dto.isRead());
        return notification;
    }
}
