use sqlx::{Connection as _, MySqlConnection};

#[path = "../src/queries.rs"]
mod queries;

use queries::{CreateEnumUserParams, EnumUsersStatus};

async fn setup() -> Result<MySqlConnection, Box<dyn std::error::Error>> {
    let db_url = std::env::var("DATABASE_URL")?;
    let mut conn = MySqlConnection::connect(&db_url).await?;

    sqlx::query("DROP TABLE IF EXISTS enum_users")
        .execute(&mut conn)
        .await?;
    sqlx::query(
        "CREATE TABLE enum_users (
            id BIGINT NOT NULL AUTO_INCREMENT PRIMARY KEY,
            name VARCHAR(255) NOT NULL,
            status ENUM ('active', 'inactive', 'banned') NOT NULL
        )",
    )
    .execute(&mut conn)
    .await?;

    Ok(conn)
}

#[tokio::test]
async fn mysql_enum_roundtrip() -> Result<(), Box<dyn std::error::Error>> {
    let mut conn = setup().await?;

    let banned_id = {
        let mut last = 0;
        for (name, status) in [
            ("Alice", EnumUsersStatus::Active),
            ("Bob", EnumUsersStatus::Inactive),
            ("Carol", EnumUsersStatus::Banned),
        ] {
            last = queries::create_enum_user(
                &mut conn,
                CreateEnumUserParams {
                    name: name.to_string(),
                    status,
                },
            )
            .await?;
        }
        last as i64
    };

    let carol = queries::get_enum_user(&mut conn, banned_id).await?;
    assert_eq!(carol.name, "Carol");
    assert_eq!(carol.status, EnumUsersStatus::Banned);

    let active = queries::list_enum_users_by_status(&mut conn, EnumUsersStatus::Active).await?;
    assert_eq!(
        active.iter().map(|r| r.name.as_str()).collect::<Vec<_>>(),
        ["Alice"]
    );

    Ok(())
}
