-- name: BulkCreateCopyAuthors :copyfrom
INSERT INTO copy_authors (name, bio)
VALUES (?, ?);

-- name: ListCopyAuthors :many
SELECT id, name, bio
FROM copy_authors
ORDER BY id;
