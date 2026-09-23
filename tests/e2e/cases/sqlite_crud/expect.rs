use sqlx::{Connection as _, SqliteConnection};

#[path = "../src/queries.rs"]
mod queries;

use queries::{CreateAuthorParams, UpdateAuthorBioParams};

async fn setup() -> Result<SqliteConnection, Box<dyn std::error::Error>> {
    let db_url = std::env::var("DATABASE_URL")?;
    let mut conn = SqliteConnection::connect(&db_url).await?;

    sqlx::query("DROP TABLE IF EXISTS authors")
        .execute(&mut conn)
        .await?;
    sqlx::query(
        "CREATE TABLE authors (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            name TEXT NOT NULL,
            bio TEXT,
            rating REAL NOT NULL,
            active BOOLEAN NOT NULL,
            avatar BLOB,
            created_at DATETIME NOT NULL
        )",
    )
    .execute(&mut conn)
    .await?;

    Ok(conn)
}

fn created_at() -> chrono::NaiveDateTime {
    chrono::NaiveDate::from_ymd_opt(2024, 1, 2)
        .unwrap()
        .and_hms_opt(3, 4, 5)
        .unwrap()
}

fn author(name: &str, bio: Option<&str>) -> CreateAuthorParams {
    CreateAuthorParams {
        name: name.to_string(),
        bio: bio.map(str::to_string),
        rating: 4.25,
        active: true,
        avatar: Some(vec![0xde, 0xad, 0xbe, 0xef]),
        created_at: created_at(),
    }
}

#[tokio::test]
async fn sqlite_crud_roundtrip() -> Result<(), Box<dyn std::error::Error>> {
    let mut conn = setup().await?;

    // :execlastid returns SQLite's rowid for the inserted row.
    let first_id = queries::create_author(&mut conn, author("Alice", Some("Rust enthusiast"))).await?;
    let second_id = queries::create_author(&mut conn, author("Bob", None)).await?;
    assert_eq!(second_id, first_id + 1);

    let fetched = queries::get_author(&mut conn, first_id).await?;
    assert_eq!(fetched.name, "Alice");
    assert_eq!(fetched.bio.as_deref(), Some("Rust enthusiast"));
    assert_eq!(fetched.rating, 4.25);
    assert!(fetched.active);
    assert_eq!(fetched.avatar.as_deref(), Some(&[0xde, 0xad, 0xbe, 0xef][..]));
    assert_eq!(fetched.created_at, created_at());

    let all = queries::list_authors(&mut conn).await?;
    assert_eq!(all.len(), 2);
    assert_eq!(all[1].bio, None);

    // :execrows reports the affected-row count.
    let updated = queries::update_author_bio(
        &mut conn,
        UpdateAuthorBioParams {
            bio: Some("Now with a bio".to_string()),
            id: second_id,
        },
    )
    .await?;
    assert_eq!(updated, 1);
    let fetched = queries::get_author(&mut conn, second_id).await?;
    assert_eq!(fetched.bio.as_deref(), Some("Now with a bio"));

    // :execresult hands back the driver's own result type.
    let result: sqlx::sqlite::SqliteQueryResult = queries::delete_author(&mut conn, second_id).await?;
    assert_eq!(result.rows_affected(), 1);

    queries::truncate_authors(&mut conn).await?;
    assert!(queries::list_authors(&mut conn).await?.is_empty());

    Ok(())
}
