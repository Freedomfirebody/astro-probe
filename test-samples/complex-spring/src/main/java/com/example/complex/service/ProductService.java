package com.example.complex.service;

import com.example.complex.model.Product;
import com.example.complex.model.enums.ProductStatus;

import java.math.BigDecimal;
import java.util.List;
import java.util.Optional;

public interface ProductService {

    Product createProduct(String name, BigDecimal price, Integer stock);

    Optional<Product> findById(Long id);

    List<Product> findAll();

    List<Product> findByStatus(ProductStatus status);

    List<Product> findLowStockProducts(int threshold);

    Product updateProduct(Long id, String name, BigDecimal price, Integer stock, ProductStatus status);

    void updateStock(Long productId, int quantityChange);

    void decrementStock(Long productId, int quantity);

    void restoreStock(Long productId, int quantity);

    void deleteProduct(Long id);
}
