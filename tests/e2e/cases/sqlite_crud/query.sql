-- name: CreateAuthor :execlastid
INSERT INTO authors (name, bio, rating, active, avatar, created_at)
VALUES (?, ?, ?, ?, ?, ?);

-- name: GetAuthor :one
SELECT id, name, bio, rating, active, avatar, created_at
FROM authors
WHERE id = ?;

-- name: ListAuthors :many
SELECT id, name, bio, rating, active, avatar, created_at
FROM authors
ORDER BY id;

-- name: UpdateAuthorBio :execrows
UPDATE authors
SET bio = ?
WHERE id = ?;

-- name: DeleteAuthor :execresult
DELETE FROM authors
WHERE id = ?;

-- name: TruncateAuthors :exec
DELETE FROM authors;
