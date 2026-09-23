CREATE TABLE enum_users (
    id BIGINT NOT NULL AUTO_INCREMENT PRIMARY KEY,
    name VARCHAR(255) NOT NULL,
    status ENUM ('active', 'inactive', 'banned') NOT NULL
);
