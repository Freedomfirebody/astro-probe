package com.example.medium.service.impl;

import com.example.medium.dto.UserDto;
import com.example.medium.exception.ResourceNotFoundException;
import com.example.medium.mapper.UserMapper;
import com.example.medium.model.User;
import com.example.medium.repository.UserRepository;
import com.example.medium.service.UserService;
import com.example.medium.service.base.BaseService;

import org.springframework.beans.factory.annotation.Autowired;
import org.springframework.stereotype.Service;
import org.springframework.transaction.annotation.Transactional;

import java.util.List;
import java.util.Optional;

@Service
@Transactional
public class UserServiceImpl extends BaseService<User, Long> implements UserService {

    private final UserRepository userRepository;
    private final UserMapper userMapper;

    @Autowired
    public UserServiceImpl(UserRepository userRepository, UserMapper userMapper) {
        this.userRepository = userRepository;
        this.userMapper = userMapper;
    }

    @Override
    public User createUser(UserDto userDto) {
        User user = UserMapper.toEntity(userDto);
        User savedUser = userRepository.save(user);
        logCreation("User", savedUser.getId());
        return savedUser;
    }

    @Override
    @Transactional(readOnly = true)
    public User findById(Long id) {
        logRetrieval("User", id);
        return userRepository.findById(id)
                .orElseThrow(() -> new ResourceNotFoundException("User", "id", id));
    }

    @Override
    @Transactional(readOnly = true)
    public Optional<User> findByUsername(String username) {
        return userRepository.findByUsername(username);
    }

    @Override
    @Transactional(readOnly = true)
    public List<User> findAll() {
        return userRepository.findAll();
    }

    @Override
    public User updateUser(Long id, UserDto userDto) {
        User existingUser = findById(id);
        existingUser.setUsername(userDto.getUsername());
        existingUser.setEmail(userDto.getEmail());
        existingUser.setRole(userDto.getRole());
        User updatedUser = userRepository.save(existingUser);
        logUpdate("User", updatedUser.getId());
        return updatedUser;
    }

    @Override
    public void deleteUser(Long id) {
        User user = findById(id);
        userRepository.delete(user);
        logDeletion("User", id);
    }
}
