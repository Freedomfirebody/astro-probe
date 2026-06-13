package com.example.simple.service;

import com.example.simple.dto.UserDto;
import com.example.simple.exception.ResourceNotFoundException;
import com.example.simple.model.User;
import com.example.simple.repository.UserRepository;
import com.example.simple.util.StringUtils;
import org.springframework.beans.factory.annotation.Autowired;
import org.springframework.stereotype.Service;

import java.util.List;
import java.util.stream.Collectors;

@Service
public class UserServiceImpl implements UserService {

    private final UserRepository userRepository;

    @Autowired
    public UserServiceImpl(UserRepository userRepository) {
        this.userRepository = userRepository;
    }

    @Override
    public List<UserDto> findAll() {
        return userRepository.findAll()
                .stream()
                .map(this::toDto)
                .collect(Collectors.toList());
    }

    @Override
    public UserDto findById(Long id) {
        User user = userRepository.findById(id)
                .orElseThrow(() -> new ResourceNotFoundException("User not found with id: " + id));
        return toDto(user);
    }

    @Override
    public UserDto create(UserDto userDto) {
        User user = toEntity(userDto);
        User saved = userRepository.save(user);
        return toDto(saved);
    }

    @Override
    public UserDto update(Long id, UserDto userDto) {
        User existing = userRepository.findById(id)
                .orElseThrow(() -> new ResourceNotFoundException("User not found with id: " + id));

        existing.setName(StringUtils.capitalize(userDto.getName()));
        existing.setEmail(StringUtils.formatEmail(userDto.getEmail()));

        User updated = userRepository.save(existing);
        return toDto(updated);
    }

    @Override
    public void delete(Long id) {
        if (!userRepository.existsById(id)) {
            throw new ResourceNotFoundException("User not found with id: " + id);
        }
        userRepository.deleteById(id);
    }

    private UserDto toDto(User user) {
        return new UserDto(user.getName(), user.getEmail());
    }

    private User toEntity(UserDto dto) {
        String name = StringUtils.capitalize(dto.getName());
        String email = StringUtils.formatEmail(dto.getEmail());
        return new User(name, email);
    }
}
