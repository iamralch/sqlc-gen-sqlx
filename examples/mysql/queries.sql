-- name: CreateAuthor :execlastid
INSERT INTO authors (name, bio, status)
VALUES (?, ?, ?);

-- name: GetAuthor :one
SELECT id, name, bio, status
FROM authors
WHERE id = ?;

-- name: ListAuthorsByIds :many
SELECT id, name, bio, status
FROM authors
WHERE id IN (sqlc.slice(ids))
ORDER BY id;

-- name: DeleteAuthorRows :execrows
DELETE FROM authors
WHERE id = ?;
