-- name: CreateSliceAuthor :execlastid
INSERT INTO slice_authors (name, country)
VALUES (?, ?);

-- name: ListAuthorsByIds :many
SELECT id, name, country
FROM slice_authors
WHERE id IN (sqlc.slice(ids))
ORDER BY id;

-- name: ListAuthorsByIdsInCountry :many
SELECT id, name, country
FROM slice_authors
WHERE id IN (sqlc.slice(ids)) AND country = ?
ORDER BY id;
