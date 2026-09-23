use sqlx::{Connection as _, MySqlConnection};

#[path = "../src/queries.rs"]
mod queries;

use queries::{CreateAuthorParams, UpdateAuthorBioParams};

async fn setup() -> Result<MySqlConnection, Box<dyn std::error::Error>> {
    let db_url = std::env::var("DATABASE_URL")?;
    let mut conn = MySqlConnection::connect(&db_url).await?;

    sqlx::query("DROP TABLE IF EXISTS authors")
        .execute(&mut conn)
        .await?;
    sqlx::query(
        "CREATE TABLE authors (
            id BIGINT NOT NULL AUTO_INCREMENT PRIMARY KEY,
            name VARCHAR(255) NOT NULL,
            bio TEXT,
            rating DECIMAL(4, 2) NOT NULL,
            born DATE,
            created_at DATETIME NOT NULL
        )",
    )
    .execute(&mut conn)
    .await?;

    Ok(conn)
}

fn author(name: &str, bio: Option<&str>) -> CreateAuthorParams {
    CreateAuthorParams {
        name: name.to_string(),
        bio: bio.map(str::to_string),
        rating: "4.25".parse().expect("decimal literal"),
        born: chrono::NaiveDate::from_ymd_opt(1980, 3, 14),
        created_at: chrono::NaiveDate::from_ymd_opt(2024, 1, 2)
            .unwrap()
            .and_hms_opt(3, 4, 5)
            .unwrap(),
    }
}

#[tokio::test]
async fn mysql_crud_roundtrip() -> Result<(), Box<dyn std::error::Error>> {
    let mut conn = setup().await?;

    // :execlastid returns MySQL's generated AUTO_INCREMENT key.
    let first_id = queries::create_author(&mut conn, author("Alice", Some("Rust enthusiast"))).await?;
    let second_id = queries::create_author(&mut conn, author("Bob", None)).await?;
    assert_eq!(second_id, first_id + 1);

    let fetched = queries::get_author(&mut conn, first_id as i64).await?;
    assert_eq!(fetched.name, "Alice");
    assert_eq!(fetched.bio.as_deref(), Some("Rust enthusiast"));
    assert_eq!(fetched.rating.to_string(), "4.25");
    assert_eq!(
        fetched.born,
        chrono::NaiveDate::from_ymd_opt(1980, 3, 14)
    );
    assert_eq!(
        fetched.created_at,
        chrono::NaiveDate::from_ymd_opt(2024, 1, 2)
            .unwrap()
            .and_hms_opt(3, 4, 5)
            .unwrap()
    );

    let all = queries::list_authors(&mut conn).await?;
    assert_eq!(all.len(), 2);
    assert_eq!(all[1].bio, None);

    // :execrows reports the affected-row count.
    let updated = queries::update_author_bio(
        &mut conn,
        UpdateAuthorBioParams {
            bio: Some("Now with a bio".to_string()),
            id: second_id as i64,
        },
    )
    .await?;
    assert_eq!(updated, 1);
    let fetched = queries::get_author(&mut conn, second_id as i64).await?;
    assert_eq!(fetched.bio.as_deref(), Some("Now with a bio"));

    // :execresult hands back the driver's own result type.
    let result: sqlx::mysql::MySqlQueryResult =
        queries::delete_author(&mut conn, second_id as i64).await?;
    assert_eq!(result.rows_affected(), 1);

    queries::truncate_authors(&mut conn).await?;
    assert!(queries::list_authors(&mut conn).await?.is_empty());

    Ok(())
}
