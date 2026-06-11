package com.example.medium.service;

import com.example.medium.dto.UserDto;
import com.example.medium.model.User;

import java.util.List;
import java.util.Optional;

public interface UserService {

    User createUser(UserDto userDto);

    User findById(Long id);

    Optional<User> findByUsername(String username);

    List<User> findAll();

    User updateUser(Long id, UserDto userDto);

    void deleteUser(Long id);
}
