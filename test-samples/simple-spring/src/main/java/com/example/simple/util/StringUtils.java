package com.example.simple.util;

public final class StringUtils {

    private StringUtils() {
        // Prevent instantiation
    }

    public static String capitalize(String input) {
        if (isEmpty(input)) {
            return input;
        }
        return input.substring(0, 1).toUpperCase() + input.substring(1);
    }

    public static boolean isEmpty(String input) {
        return input == null || input.trim().isEmpty();
    }

    public static String formatEmail(String email) {
        if (isEmpty(email)) {
            return email;
        }
        return email.trim().toLowerCase();
    }
}
