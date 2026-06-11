package com.example.medium.service.impl;

import com.example.medium.dto.ProductDto;
import com.example.medium.exception.InsufficientStockException;
import com.example.medium.exception.ResourceNotFoundException;
import com.example.medium.mapper.ProductMapper;
import com.example.medium.model.Product;
import com.example.medium.repository.ProductRepository;
import com.example.medium.service.ProductService;
import com.example.medium.service.base.BaseService;

import org.springframework.beans.factory.annotation.Autowired;
import org.springframework.stereotype.Service;
import org.springframework.transaction.annotation.Transactional;

import java.util.List;

@Service
@Transactional
public class ProductServiceImpl extends BaseService<Product, Long> implements ProductService {

    private final ProductRepository productRepository;
    private final ProductMapper productMapper;

    @Autowired
    public ProductServiceImpl(ProductRepository productRepository, ProductMapper productMapper) {
        this.productRepository = productRepository;
        this.productMapper = productMapper;
    }

    @Override
    public Product createProduct(ProductDto productDto) {
        Product product = ProductMapper.toEntity(productDto);
        product.setStock(0);
        Product savedProduct = productRepository.save(product);
        logCreation("Product", savedProduct.getId());
        return savedProduct;
    }

    @Override
    @Transactional(readOnly = true)
    public Product findById(Long id) {
        logRetrieval("Product", id);
        return productRepository.findById(id)
                .orElseThrow(() -> new ResourceNotFoundException("Product", "id", id));
    }

    @Override
    @Transactional(readOnly = true)
    public List<Product> findAll() {
        return productRepository.findAll();
    }

    @Override
    @Transactional(readOnly = true)
    public List<Product> findByCategory(String category) {
        return productRepository.findByCategory(category);
    }

    @Override
    public Product updateProduct(Long id, ProductDto productDto) {
        Product existingProduct = findById(id);
        existingProduct.setName(productDto.getName());
        existingProduct.setPrice(productDto.getPrice());
        existingProduct.setCategory(productDto.getCategory());
        Product updatedProduct = productRepository.save(existingProduct);
        logUpdate("Product", updatedProduct.getId());
        return updatedProduct;
    }

    @Override
    public void updateStock(Long productId, int quantity) {
        Product product = findById(productId);
        int newStock = product.getStock() + quantity;
        if (newStock < 0) {
            throw new InsufficientStockException(product.getName(), product.getStock(), Math.abs(quantity));
        }
        product.setStock(newStock);
        productRepository.save(product);
        logger.info("Updated stock for product {}: {} -> {}", productId, product.getStock() - quantity, newStock);
    }

    @Override
    public void deleteProduct(Long id) {
        Product product = findById(id);
        productRepository.delete(product);
        logDeletion("Product", id);
    }
}
