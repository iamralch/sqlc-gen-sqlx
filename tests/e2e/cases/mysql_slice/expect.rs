use sqlx::{Connection as _, MySqlConnection};

#[path = "../src/queries.rs"]
mod queries;

use queries::{CreateSliceAuthorParams, ListAuthorsByIdsInCountryParams};

async fn setup() -> Result<MySqlConnection, Box<dyn std::error::Error>> {
    let db_url = std::env::var("DATABASE_URL")?;
    let mut conn = MySqlConnection::connect(&db_url).await?;

    sqlx::query("DROP TABLE IF EXISTS slice_authors")
        .execute(&mut conn)
        .await?;
    sqlx::query(
        "CREATE TABLE slice_authors (
            id BIGINT NOT NULL AUTO_INCREMENT PRIMARY KEY,
            name VARCHAR(255) NOT NULL,
            country VARCHAR(2) NOT NULL
        )",
    )
    .execute(&mut conn)
    .await?;

    Ok(conn)
}

#[tokio::test]
async fn mysql_slice_expands_to_one_placeholder_per_element()
-> Result<(), Box<dyn std::error::Error>> {
    let mut conn = setup().await?;

    let mut ids = Vec::new();
    for (name, country) in [("Alice", "GB"), ("Bob", "US"), ("Carol", "GB")] {
        let id = queries::create_slice_author(
            &mut conn,
            CreateSliceAuthorParams {
                name: name.to_string(),
                country: country.to_string(),
            },
        )
        .await?;
        ids.push(id as i64);
    }

    let found = queries::list_authors_by_ids(&mut conn, vec![ids[0], ids[2]]).await?;
    assert_eq!(
        found.iter().map(|r| r.name.as_str()).collect::<Vec<_>>(),
        ["Alice", "Carol"]
    );

    // A single-element slice still has to bind exactly one placeholder.
    let found = queries::list_authors_by_ids(&mut conn, vec![ids[1]]).await?;
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].name, "Bob");

    // An empty slice becomes `IN (NULL)`, which matches nothing.
    let found = queries::list_authors_by_ids(&mut conn, vec![]).await?;
    assert!(found.is_empty());

    // The slice comes first in the text but sqlc numbers it last, so this only
    // passes if the generated binds follow the query text.
    let found = queries::list_authors_by_ids_in_country(
        &mut conn,
        ListAuthorsByIdsInCountryParams {
            id: ids.clone(),
            country: "GB".to_string(),
        },
    )
    .await?;
    assert_eq!(
        found.iter().map(|r| r.name.as_str()).collect::<Vec<_>>(),
        ["Alice", "Carol"]
    );

    Ok(())
}
