package com.example.complex.service.impl;

import com.example.complex.async.EmailService;
import com.example.complex.event.StockDepletedEvent;
import com.example.complex.exception.InsufficientStockException;
import com.example.complex.exception.ResourceNotFoundException;
import com.example.complex.model.Product;
import com.example.complex.model.enums.ProductStatus;
import com.example.complex.repository.ProductRepository;
import com.example.complex.service.ProductService;
import org.slf4j.Logger;
import org.slf4j.LoggerFactory;
import org.springframework.context.ApplicationEventPublisher;
import org.springframework.stereotype.Service;
import org.springframework.transaction.annotation.Transactional;

import java.math.BigDecimal;
import java.util.List;
import java.util.Optional;

@Service
@Transactional
public class ProductServiceImpl implements ProductService {

    private static final Logger logger = LoggerFactory.getLogger(ProductServiceImpl.class);

    private final ProductRepository productRepository;
    private final ApplicationEventPublisher eventPublisher;
    private final EmailService emailService;

    public ProductServiceImpl(ProductRepository productRepository,
                              ApplicationEventPublisher eventPublisher,
                              EmailService emailService) {
        this.productRepository = productRepository;
        this.eventPublisher = eventPublisher;
        this.emailService = emailService;
    }

    @Override
    public Product createProduct(String name, BigDecimal price, Integer stock) {
        logger.info("Creating product: {} (price: {}, stock: {})", name, price, stock);
        Product product = new Product(name, price, stock, ProductStatus.ACTIVE);
        return productRepository.save(product);
    }

    @Override
    @Transactional(readOnly = true)
    public Optional<Product> findById(Long id) {
        return productRepository.findById(id);
    }

    @Override
    @Transactional(readOnly = true)
    public List<Product> findAll() {
        return productRepository.findAll();
    }

    @Override
    @Transactional(readOnly = true)
    public List<Product> findByStatus(ProductStatus status) {
        return productRepository.findByStatus(status);
    }

    @Override
    @Transactional(readOnly = true)
    public List<Product> findLowStockProducts(int threshold) {
        return productRepository.findLowStockProducts(threshold);
    }

    @Override
    public Product updateProduct(Long id, String name, BigDecimal price, Integer stock, ProductStatus status) {
        Product product = productRepository.findById(id)
                .orElseThrow(() -> new ResourceNotFoundException("Product", "id", id));

        product.setName(name);
        product.setPrice(price);
        product.setStock(stock);
        product.setStatus(status);

        return productRepository.save(product);
    }

    @Override
    public void updateStock(Long productId, int quantityChange) {
        Product product = productRepository.findById(productId)
                .orElseThrow(() -> new ResourceNotFoundException("Product", "id", productId));

        int newStock = product.getStock() + quantityChange;
        if (newStock < 0) {
            throw new InsufficientStockException(productId, Math.abs(quantityChange), product.getStock());
        }

        product.setStock(newStock);
        if (newStock == 0) {
            product.setStatus(ProductStatus.OUT_OF_STOCK);
            // Publish StockDepletedEvent when stock hits 0
            eventPublisher.publishEvent(new StockDepletedEvent(this, productId, product.getName()));
            // Also trigger async email alert
            emailService.sendStockAlert(product);
            logger.warn("Product {} is now out of stock!", product.getName());
        }

        productRepository.save(product);
        logger.info("Stock updated for product {}: new stock = {}", productId, newStock);
    }

    @Override
    public void decrementStock(Long productId, int quantity) {
        logger.info("Decrementing stock for product {}: quantity = {}", productId, quantity);
        updateStock(productId, -quantity);
    }

    @Override
    public void restoreStock(Long productId, int quantity) {
        logger.info("Restoring stock for product {}: quantity = {}", productId, quantity);
        Product product = productRepository.findById(productId)
                .orElseThrow(() -> new ResourceNotFoundException("Product", "id", productId));

        product.setStock(product.getStock() + quantity);
        if (product.getStatus() == ProductStatus.OUT_OF_STOCK) {
            product.setStatus(ProductStatus.ACTIVE);
        }
        productRepository.save(product);
        logger.info("Stock restored for product {}: new stock = {}", productId, product.getStock());
    }

    @Override
    public void deleteProduct(Long id) {
        if (!productRepository.existsById(id)) {
            throw new ResourceNotFoundException("Product", "id", id);
        }
        productRepository.deleteById(id);
        logger.info("Product deleted: {}", id);
    }
}
