package com.example.medium.service.base;

import org.slf4j.Logger;
import org.slf4j.LoggerFactory;

public abstract class BaseService<T, ID> {

    protected final Logger logger = LoggerFactory.getLogger(getClass());

    protected void logCreation(String entityName, ID id) {
        logger.info("Created {} with id: {}", entityName, id);
    }

    protected void logUpdate(String entityName, ID id) {
        logger.info("Updated {} with id: {}", entityName, id);
    }

    protected void logDeletion(String entityName, ID id) {
        logger.info("Deleted {} with id: {}", entityName, id);
    }

    protected void logRetrieval(String entityName, ID id) {
        logger.debug("Retrieved {} with id: {}", entityName, id);
    }

    protected void logError(String entityName, String operation, Exception e) {
        logger.error("Error performing {} on {}: {}", operation, entityName, e.getMessage(), e);
    }
}
