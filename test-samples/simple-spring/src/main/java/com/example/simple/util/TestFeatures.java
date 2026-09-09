package com.example.simple.util;

import java.util.List;

public class TestFeatures {

    static {
        int x = 10;
    }

    public <T> T genericMethod(T input) {
        return input;
    }

    public void processList(List<String> list) {
        String first = list.get(0);
    }

    public void overload() {
    }

    public void overload(String str) {
    }

    public void overload(int num) {
    }

    public void overload(String str, int num) {
    }
}
