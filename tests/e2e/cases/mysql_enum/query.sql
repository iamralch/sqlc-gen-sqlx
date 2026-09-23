-- name: CreateEnumUser :execlastid
INSERT INTO enum_users (name, status)
VALUES (?, ?);

-- name: GetEnumUser :one
SELECT id, name, status
FROM enum_users
WHERE id = ?;

-- name: ListEnumUsersByStatus :many
SELECT id, name, status
FROM enum_users
WHERE status = ?
ORDER BY id;
