#[cfg(test)]
use sqlx::{Connection as _, MySqlConnection};

#[path = "queries.rs"]
#[cfg(test)]
mod queries;
#[cfg(test)]
use queries::{AuthorsStatus, CreateAuthorParams};

#[cfg(test)]
#[tokio::test]
async fn test_author_roundtrip() {
    let db_url = std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "mysql://root@localhost:3306/sqlc_test".to_string());
    let mut conn = MySqlConnection::connect(&db_url).await.expect("connect");

    sqlx::query("DROP TABLE IF EXISTS authors")
        .execute(&mut conn)
        .await
        .unwrap();
    sqlx::query(
        "CREATE TABLE authors (
            id BIGINT NOT NULL AUTO_INCREMENT PRIMARY KEY,
            name VARCHAR(255) NOT NULL,
            bio TEXT,
            status ENUM ('active', 'retired') NOT NULL
        )",
    )
    .execute(&mut conn)
    .await
    .unwrap();

    // `:execlastid` returns the AUTO_INCREMENT key MySQL generated.
    let id = queries::create_author(
        &mut conn,
        CreateAuthorParams {
            name: "Alice".to_string(),
            bio: Some("Loves Rust".to_string()),
            status: AuthorsStatus::Active,
        },
    )
    .await
    .expect("create") as i64;

    let fetched = queries::get_author(&mut conn, id).await.expect("get");
    assert_eq!(fetched.name, "Alice");
    assert_eq!(fetched.bio.as_deref(), Some("Loves Rust"));
    assert_eq!(fetched.status, AuthorsStatus::Active);

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
