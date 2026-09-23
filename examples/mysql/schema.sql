CREATE TABLE authors (
    id BIGINT NOT NULL AUTO_INCREMENT PRIMARY KEY,
    name VARCHAR(255) NOT NULL,
    bio TEXT,
    status ENUM ('active', 'retired') NOT NULL
);
