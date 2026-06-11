package com.example.complex.dto;

import java.time.LocalDateTime;

public class NotificationDto {

    private Long id;
    private Long userId;
    private String type;
    private String message;
    private LocalDateTime sentAt;
    private boolean read;

    public NotificationDto() {
    }

    public NotificationDto(Long id, Long userId, String type, String message, LocalDateTime sentAt, boolean read) {
        this.id = id;
        this.userId = userId;
        this.type = type;
        this.message = message;
        this.sentAt = sentAt;
        this.read = read;
    }

    public Long getId() {
        return id;
    }

    public void setId(Long id) {
        this.id = id;
    }

    public Long getUserId() {
        return userId;
    }

    public void setUserId(Long userId) {
        this.userId = userId;
    }

    public String getType() {
        return type;
    }

    public void setType(String type) {
        this.type = type;
    }

    public String getMessage() {
        return message;
    }

    public void setMessage(String message) {
        this.message = message;
    }

    public LocalDateTime getSentAt() {
        return sentAt;
    }

    public void setSentAt(LocalDateTime sentAt) {
        this.sentAt = sentAt;
    }

    public boolean isRead() {
        return read;
    }

    public void setRead(boolean read) {
        this.read = read;
    }
}
