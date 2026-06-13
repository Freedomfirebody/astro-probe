package com.example.complex.service.impl;

import com.example.complex.exception.ResourceNotFoundException;
import com.example.complex.model.User;
import com.example.complex.repository.UserRepository;
import com.example.complex.service.NotificationService;
import com.example.complex.service.UserService;
import org.slf4j.Logger;
import org.slf4j.LoggerFactory;
import org.springframework.stereotype.Service;
import org.springframework.transaction.annotation.Transactional;

import java.util.List;
import java.util.Optional;

@Service
@Transactional
public class UserServiceImpl implements UserService {

    private static final Logger logger = LoggerFactory.getLogger(UserServiceImpl.class);

    private final UserRepository userRepository;
    private final NotificationService notificationService;

    public UserServiceImpl(UserRepository userRepository, NotificationService notificationService) {
        this.userRepository = userRepository;
        this.notificationService = notificationService;
    }

    @Override
    public User createUser(String name, String email, String role) {
        logger.info("Creating user: {} ({})", name, email);
        if (userRepository.existsByEmail(email)) {
            throw new IllegalArgumentException("User with email " + email + " already exists");
        }
        User user = new User(name, email, role);
        User savedUser = userRepository.save(user);

        // Cross-call: notify the user about account creation
        notificationService.notifyUser(savedUser.getId(), "ACCOUNT_CREATED",
                "Welcome, " + name + "! Your account has been created.");

        logger.info("User created with ID: {}", savedUser.getId());
        return savedUser;
    }

    @Override
    @Transactional(readOnly = true)
    public Optional<User> findById(Long id) {
        return userRepository.findById(id);
    }

    @Override
    @Transactional(readOnly = true)
    public Optional<User> findByEmail(String email) {
        return userRepository.findByEmail(email);
    }

    @Override
    @Transactional(readOnly = true)
    public List<User> findAll() {
        return userRepository.findAll();
    }

    @Override
    @Transactional(readOnly = true)
    public List<User> findByRole(String role) {
        return userRepository.findByRole(role);
    }

    @Override
    @Transactional(readOnly = true)
    public List<User> searchByName(String keyword) {
        return userRepository.searchByName(keyword);
    }

    @Override
    public User updateUser(Long id, String name, String email, String role) {
        User user = userRepository.findById(id)
                .orElseThrow(() -> new ResourceNotFoundException("User", "id", id));

        user.setName(name);
        user.setEmail(email);
        user.setRole(role);

        User updatedUser = userRepository.save(user);
        logger.info("User updated: {}", id);
        return updatedUser;
    }

    @Override
    public void deleteUser(Long id) {
        if (!userRepository.existsById(id)) {
            throw new ResourceNotFoundException("User", "id", id);
        }
        userRepository.deleteById(id);
        logger.info("User deleted: {}", id);
    }
}
