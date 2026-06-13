package com.example.complex.callback;

public interface ProcessingCallback<T> {

    void onSuccess(T result);

    void onFailure(Exception e);

    void onProgress(int percent);
}
