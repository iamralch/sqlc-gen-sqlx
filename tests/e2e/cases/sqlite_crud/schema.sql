CREATE TABLE authors (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL,
    bio TEXT,
    rating REAL NOT NULL,
    active BOOLEAN NOT NULL,
    avatar BLOB,
    created_at DATETIME NOT NULL
);
