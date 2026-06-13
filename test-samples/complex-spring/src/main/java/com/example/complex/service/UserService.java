package com.example.complex.service;

import com.example.complex.model.User;

import java.util.List;
import java.util.Optional;

public interface UserService {

    User createUser(String name, String email, String role);

    Optional<User> findById(Long id);

    Optional<User> findByEmail(String email);

    List<User> findAll();

    List<User> findByRole(String role);

    List<User> searchByName(String keyword);

    User updateUser(Long id, String name, String email, String role);

    void deleteUser(Long id);
}
