CREATE TABLE authors (
    id BIGINT NOT NULL AUTO_INCREMENT PRIMARY KEY,
    name VARCHAR(255) NOT NULL,
    bio TEXT,
    rating DECIMAL(4, 2) NOT NULL,
    born DATE,
    created_at DATETIME NOT NULL
);
