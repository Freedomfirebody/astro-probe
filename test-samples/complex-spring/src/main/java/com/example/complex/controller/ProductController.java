package com.example.complex.controller;

import com.example.complex.dto.ProductDto;
import com.example.complex.exception.ResourceNotFoundException;
import com.example.complex.mapper.EntityMapper;
import com.example.complex.model.Product;
import com.example.complex.model.enums.ProductStatus;
import com.example.complex.service.ProductService;
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
import java.util.stream.Collectors;

@RestController
@RequestMapping("/api/products")
public class ProductController {

    private final ProductService productService;
    private final EntityMapper entityMapper;

    public ProductController(ProductService productService, EntityMapper entityMapper) {
        this.productService = productService;
        this.entityMapper = entityMapper;
    }

    @GetMapping
    public ResponseEntity<List<ProductDto>> getAllProducts() {
        List<ProductDto> products = productService.findAll().stream()
                .map(entityMapper::toProductDto)
                .collect(Collectors.toList());
        return ResponseEntity.ok(products);
    }

    @GetMapping("/{id}")
    public ResponseEntity<ProductDto> getProductById(@PathVariable Long id) {
        Product product = productService.findById(id)
                .orElseThrow(() -> new ResourceNotFoundException("Product", "id", id));
        return ResponseEntity.ok(entityMapper.toProductDto(product));
    }

    @GetMapping("/status/{status}")
    public ResponseEntity<List<ProductDto>> getProductsByStatus(@PathVariable ProductStatus status) {
        List<ProductDto> products = productService.findByStatus(status).stream()
                .map(entityMapper::toProductDto)
                .collect(Collectors.toList());
        return ResponseEntity.ok(products);
    }

    @GetMapping("/low-stock")
    public ResponseEntity<List<ProductDto>> getLowStockProducts(@RequestParam(defaultValue = "5") int threshold) {
        List<ProductDto> products = productService.findLowStockProducts(threshold).stream()
                .map(entityMapper::toProductDto)
                .collect(Collectors.toList());
        return ResponseEntity.ok(products);
    }

    @PostMapping
    public ResponseEntity<ProductDto> createProduct(@RequestBody ProductDto productDto) {
        Product product = productService.createProduct(
                productDto.getName(), productDto.getPrice(), productDto.getStock());
        return ResponseEntity.status(HttpStatus.CREATED).body(entityMapper.toProductDto(product));
    }

    @PutMapping("/{id}")
    public ResponseEntity<ProductDto> updateProduct(@PathVariable Long id, @RequestBody ProductDto productDto) {
        ProductStatus status = ProductStatus.valueOf(productDto.getStatus());
        Product product = productService.updateProduct(
                id, productDto.getName(), productDto.getPrice(), productDto.getStock(), status);
        return ResponseEntity.ok(entityMapper.toProductDto(product));
    }

    @PutMapping("/{id}/stock")
    public ResponseEntity<Void> updateStock(@PathVariable Long id, @RequestParam int quantity) {
        productService.updateStock(id, quantity);
        return ResponseEntity.ok().build();
    }

    @DeleteMapping("/{id}")
    public ResponseEntity<Void> deleteProduct(@PathVariable Long id) {
        productService.deleteProduct(id);
        return ResponseEntity.noContent().build();
    }
}
