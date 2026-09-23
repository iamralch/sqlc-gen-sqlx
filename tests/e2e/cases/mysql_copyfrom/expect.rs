use sqlx::{Connection as _, MySqlConnection};

#[path = "../src/queries.rs"]
mod queries;

use queries::BulkCreateCopyAuthorsParams;

async fn setup() -> Result<MySqlConnection, Box<dyn std::error::Error>> {
    let db_url = std::env::var("DATABASE_URL")?;
    let mut conn = MySqlConnection::connect(&db_url).await?;

    sqlx::query("DROP TABLE IF EXISTS copy_authors")
        .execute(&mut conn)
        .await?;
    sqlx::query(
        "CREATE TABLE copy_authors (
            id BIGINT NOT NULL AUTO_INCREMENT PRIMARY KEY,
            name VARCHAR(255) NOT NULL,
            bio TEXT
        )",
    )
    .execute(&mut conn)
    .await?;

    Ok(conn)
}

#[tokio::test]
async fn mysql_copyfrom_inserts_every_row() -> Result<(), Box<dyn std::error::Error>> {
    let mut conn = setup().await?;

    let items = (0..250)
        .map(|i| BulkCreateCopyAuthorsParams {
            name: format!("author-{i:03}"),
            bio: (i % 2 == 0).then(|| format!("bio-{i}")),
        })
        .collect::<Vec<_>>();

    let inserted = queries::bulk_create_copy_authors(&mut conn, items).await?;
    assert_eq!(inserted, 250);

    let rows = queries::list_copy_authors(&mut conn).await?;
    assert_eq!(rows.len(), 250);
    assert_eq!(rows[0].name, "author-000");
    assert_eq!(rows[0].bio.as_deref(), Some("bio-0"));
    assert_eq!(rows[1].bio, None);
    assert_eq!(rows[249].name, "author-249");

    // An empty iterator must not issue a statement at all.
    let inserted = queries::bulk_create_copy_authors(&mut conn, Vec::new()).await?;
    assert_eq!(inserted, 0);

    Ok(())
}
