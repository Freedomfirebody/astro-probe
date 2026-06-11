package com.example.medium.service;

import com.example.medium.dto.ProductDto;
import com.example.medium.model.Product;

import java.util.List;

public interface ProductService {

    Product createProduct(ProductDto productDto);

    Product findById(Long id);

    List<Product> findAll();

    List<Product> findByCategory(String category);

    Product updateProduct(Long id, ProductDto productDto);

    void updateStock(Long productId, int quantity);

    void deleteProduct(Long id);
}
