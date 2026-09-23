#[cfg(test)]
use sqlx::{Connection as _, SqliteConnection};

#[path = "queries.rs"]
#[cfg(test)]
mod queries;
#[cfg(test)]
use queries::CreateAuthorParams;

#[cfg(test)]
#[tokio::test]
async fn test_author_roundtrip() {
    // An in-memory database needs no setup and no cleanup.
    let db_url = std::env::var("DATABASE_URL").unwrap_or_else(|_| "sqlite::memory:".to_string());
    let mut conn = SqliteConnection::connect(&db_url).await.expect("connect");

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS authors (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            name TEXT NOT NULL,
            bio TEXT,
            rating REAL NOT NULL
        )",
    )
    .execute(&mut conn)
    .await
    .unwrap();
    sqlx::query("DELETE FROM authors")
        .execute(&mut conn)
        .await
        .unwrap();

    // `:execlastid` returns the rowid SQLite assigned.
    let id = queries::create_author(
        &mut conn,
        CreateAuthorParams {
            name: "Alice".to_string(),
            bio: Some("Loves Rust".to_string()),
            rating: 4.5,
        },
    )
    .await
    .expect("create");

    let fetched = queries::get_author(&mut conn, id).await.expect("get");
    assert_eq!(fetched.name, "Alice");
    assert_eq!(fetched.bio.as_deref(), Some("Loves Rust"));
    assert_eq!(fetched.rating, 4.5);

    // `sqlc.slice()` expands to one `?` per element at call time.
    let listed = queries::list_authors_by_ids(&mut conn, vec![id])
        .await
        .expect("list");
    assert_eq!(listed.len(), 1);

    let rows = queries::delete_author_rows(&mut conn, id)
        .await
        .expect("delete");
    assert_eq!(rows, 1);
}

fn main() {}
