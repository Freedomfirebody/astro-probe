package com.example.complex.async;

import com.example.complex.model.Order;
import com.example.complex.model.Product;

public interface EmailService {

    void sendOrderConfirmation(Order order);

    void sendStockAlert(Product product);

    void sendOrderCancellation(Order order);
}
