# sqlc-gen-sqlx

A [sqlc](https://sqlc.dev) plugin that generates type-safe [sqlx](https://github.com/transact-rs/sqlx) Rust code from SQL queries.

Generated code targets **sqlx 0.9 or newer**. Queries that rewrite their SQL at
run time — `sqlc.slice()` parameters that cannot use `= ANY($1)` — pass the
result through `sqlx::AssertSqlSafe`, which 0.8 does not have.

## Prebuilt installation

Release archives contain the executable and install without a Rust compiler.
Install with cargo-binstall, with source compilation disabled:

```sh
cargo binstall --disable-strategies compile sqlc-gen-sqlx
```

Or declare the GitHub release directly in `mise.toml`:

```toml
[tools]
"github:mathematic-inc/sqlc-gen-sqlx" = "latest"
```

Run `mise install` to download and activate the executable. No custom mise plugin
is required. The Cargo backend (`cargo:sqlc-gen-sqlx`) also supports these releases;
set `cargo.binstall_only = true` to reject source compilation.

| Platform | Architectures | Archive |
| --- | --- | --- |
| macOS | x64, ARM64 | `.tar.gz` |
| Linux GNU (glibc 2.35 or newer) | x64, ARM64 | `.tar.gz` |
| Linux musl | x64, ARM64 | `.tar.gz` |
| Windows MSVC | x64, ARM64 | `.zip` |

Every archive includes SHA-256 checksums and GitHub build provenance. CI builds all
eight targets and runs the extracted executables on the matching architecture.
After publication, the release workflow installs through cargo-binstall and mise
and runs both installations. A missing prebuilt binary fails the release checks.

The release also includes `sqlc-gen-sqlx.wasm`. Continue using `plugins[].wasm`
with its URL and checksum for sqlc's WASM plugin mode. The native executable
installed above is used through `plugins[].process.cmd: sqlc-gen-sqlx` instead.

## Supported engines

The plugin reads `sql[*].engine` from your sqlc config and generates for the
matching sqlx driver. Anything else is rejected with an error.

| sqlc `engine` | sqlx driver | Notes |
| --- | --- | --- |
| `postgresql` | `sqlx::Postgres` | Full support, including composite types and `= ANY($1)` array binding |
| `mysql` | `sqlx::MySql` | `ENUM` columns become Rust enums; no composite types |

Enable the matching sqlx feature in your own `Cargo.toml` (`postgres` or
`mysql`). The rest of this README uses PostgreSQL in its examples; where the two
engines differ, the difference is called out.

## What it generates

For each SQL query annotated with a sqlc command, the plugin emits:

- A `const SQL: &str` holding the query text.
- A strongly-typed row struct (`QueryNameRow`) for `:one` / `:many`.
- An optional params struct (`QueryNameParams`) when a query has 2+ parameters.
- A free `pub async fn` (or `pub fn` for batch streams) that executes the query, taking the executor as its first argument.

The executor argument is generic over the `AsExecutor` trait emitted in the same file. `AsExecutor` is implemented for the natural sqlx reference types of the target engine — for PostgreSQL that is `&PgPool`, `&mut PgConnection`, `&mut Transaction<'_, Postgres>`, `&mut PoolConnection<Postgres>`, and `&mut T` of each; for MySQL the same shapes over `MySqlPool`, `MySqlConnection` and `MySql`:

```rust
// From a pool:
let author = queries::get_author(&pool, 1).await?;

// Pool connection:
let mut conn = pool.acquire().await?;
let author = queries::get_author(&mut conn, 1).await?;

// Transaction:
let mut tx = pool.begin().await?;
queries::delete_author(&mut tx, 1).await?;
tx.commit().await?;
```

## Installation

Add the plugin to your `sqlc.yaml`:

```yaml
version: "2"
plugins:
  - name: sqlc-gen-sqlx
    wasm:
      url: https://github.com/mathematic-inc/sqlc-gen-sqlx/releases/download/v0.2.3/sqlc-gen-sqlx.wasm
      sha256: "2ff3fdf8fa12c5eb0e399eec59287989f95cdab9969ca86fa9c0c4e6fcab54e1"
sql:
  - engine: postgresql
    queries: queries.sql
    schema: schema.sql
    codegen:
      - plugin: sqlc-gen-sqlx
        out: src/
        options:
          output: queries.rs
```

## Configuration

All options are passed in `codegen[*].options`:

| Key | Type | Default | Description |
|---|---|---|---|
| `output` | string | `queries.rs` | Output filename |
| `overrides` | array | `[]` | Type overrides (`rs_type`, optional `borrowed_rs_type`; see below) |
| `row_derives` | array | `[]` | Extra derives for row and params structs |
| `enum_derives` | array | `[]` | Extra derives for generated enum types |
| `composite_derives` | array | `[]` | Extra derives for generated composite types |
| `copy_cheap_types` | array | `[]` | Type names to mark as copy-cheap |

### Type overrides

Override the Rust type used for a database column type or a specific column.
`db_type` is matched against the engine's own type names:

```yaml
options:
  overrides:
    - db_type: "timestamptz"
      rs_type: "time::OffsetDateTime"
      copy_cheap: false
    - column: "users.created_at"
      rs_type: "chrono::DateTime<chrono::Local>"
      copy_cheap: false
```

### Borrowed parameters

Add `borrowed_rs_type` to a type or column override to take that type by
reference in parameter positions. Row struct fields, array contents, and the
`Item` of `:copyfrom` chunks continue to use the owned form:

```yaml
options:
  overrides:
    - db_type: "text"
      borrowed_rs_type: "&str"
```

With that override, generated signatures borrow scalar `text` parameters and
the codegen threads lifetimes only where needed:

```rust
// Scalar — lifetime elided
pub async fn get_author_by_name<E: AsExecutor>(
    mut db: E, name: &str,
) -> Result<GetAuthorByNameRow, sqlx::Error> { ... }

// Multiple params — struct carries `'a`, fn uses `'_`
pub struct CreateAuthorParams<'a> {
    pub name: &'a str,
    pub bio: Option<&'a str>,
}
pub async fn create_author<E: AsExecutor>(
    mut db: E, arg: CreateAuthorParams<'_>,
) -> Result<CreateAuthorRow, sqlx::Error> { ... }

// Row struct stays owned — results are returned by value
pub struct GetAuthorByNameRow { pub name: String, /* ... */ }
```

`rs_type` is optional alongside `borrowed_rs_type`. Omit it to keep the
built-in owned default; set both to fully customize:

```yaml
overrides:
  - db_type: "text"
    rs_type: "MyStr"           # used for row fields & array contents
    borrowed_rs_type: "&MyStr" # used for scalar params
```

For `text[]` and `sqlc.slice(text)` the wrapper becomes a borrowed slice while
the inner item stays owned (`&[String]`), so callers can pass `&my_vec`
directly without re-collecting.

## Supported PostgreSQL types

| PostgreSQL | Rust |
|---|---|
| `bool` | `bool` |
| `int2` / `smallint` | `i16` |
| `int4` / `integer` / `int` | `i32` |
| `int8` / `bigint` | `i64` |
| `float4` / `real` | `f32` |
| `float8` / `double precision` | `f64` |
| `numeric` / `decimal` | `bigdecimal::BigDecimal` |
| `text` / `varchar` / `bpchar` / `citext` | `String` |
| `bytea` | `Vec<u8>` |
| `uuid` | `uuid::Uuid` |
| `json` / `jsonb` | `serde_json::Value` |
| `timestamptz` | `chrono::DateTime<chrono::Utc>` |
| `timestamp` | `chrono::NaiveDateTime` |
| `date` | `chrono::NaiveDate` |
| `time` | `chrono::NaiveTime` |
| `inet` / `cidr` | `ipnetwork::IpNetwork` |
| `macaddr` | `mac_address::MacAddress` |
| `hstore` | `std::collections::HashMap<String, Option<String>>` |
| `interval` | `sqlx::postgres::types::PgInterval` |
| `money` | `sqlx::postgres::types::PgMoney` |
| `oid` | `sqlx::postgres::types::Oid` |
| `int4range` | `sqlx::postgres::types::PgRange<i32>` |
| `int8range` | `sqlx::postgres::types::PgRange<i64>` |
| `numrange` | `sqlx::postgres::types::PgRange<bigdecimal::BigDecimal>` |
| `tsrange` | `sqlx::postgres::types::PgRange<chrono::NaiveDateTime>` |
| `tstzrange` | `sqlx::postgres::types::PgRange<chrono::DateTime<chrono::Utc>>` |
| `daterange` | `sqlx::postgres::types::PgRange<chrono::NaiveDate>` |
| `bit` / `varbit` | `sqlx::types::BitVec` |
| PostgreSQL ENUM | generated Rust enum |
| PostgreSQL composite | generated Rust struct |

Array types (`type[]`) become `Vec<T>`. Nullable columns become `Option<T>`.

## Supported MySQL types

| MySQL | Rust |
|---|---|
| `bool` / `boolean` | `bool` |
| `tinyint` | `i8` (`u8` when `unsigned`) |
| `smallint` | `i16` (`u16` when `unsigned`) |
| `mediumint` / `int` / `integer` | `i32` (`u32` when `unsigned`) |
| `bigint` | `i64` (`u64` when `unsigned`) |
| `serial` | `u64` |
| `year` | `u16` |
| `float` | `f32` |
| `double` / `real` | `f64` |
| `decimal` / `numeric` | `bigdecimal::BigDecimal` |
| `char` / `varchar` / `tinytext` / `text` / `mediumtext` / `longtext` | `String` |
| `binary` / `varbinary` / `blob` family / `bit` | `Vec<u8>` |
| `json` | `serde_json::Value` |
| `timestamp` | `chrono::DateTime<chrono::Utc>` |
| `datetime` | `chrono::NaiveDateTime` |
| `date` | `chrono::NaiveDate` |
| `time` | `chrono::NaiveTime` |
| MySQL ENUM | generated Rust enum |

Declared widths are ignored, so `varchar(255)` and `int(11) unsigned` resolve
the same as `varchar` and `int unsigned`. MySQL spells `bool` as `tinyint(1)`,
which is indistinguishable from a 1-byte integer in the catalog — `tinyint`
therefore maps to `i8`, and a column that really is a boolean needs a column
override:

```yaml
options:
  overrides:
    - column: "users.is_admin"
      rs_type: "bool"
```

MySQL has no composite types, so nothing is generated for them.

## Supported query annotations

| Annotation | Return type | Description |
|---|---|---|
| `:exec` | `Result<(), sqlx::Error>` | Execute, discard result |
| `:execrows` | `Result<u64, sqlx::Error>` | Execute, return rows affected |
| `:execresult` | `Result<PgQueryResult, sqlx::Error>` / `Result<MySqlQueryResult, sqlx::Error>` | Execute, return the driver's full result |
| `:execlastid` | `Result<T, sqlx::Error>` (PostgreSQL) / `Result<u64, sqlx::Error>` (MySQL) | Generated key |
| `:one` | `Result<QueryRow, sqlx::Error>` | Fetch exactly one row |
| `:many` | `Result<Vec<QueryRow>, sqlx::Error>` | Fetch all rows |
| `:batchexec` | `impl Stream<Item = Result<(), sqlx::Error>>` | Lazily execute once per item |
| `:batchone` | `impl Stream<Item = Result<QueryRow, sqlx::Error>>` | Lazily fetch one row per item |
| `:batchmany` | `impl Stream<Item = Result<Vec<QueryRow>, sqlx::Error>>` | Lazily fetch all rows per item |
| `:copyfrom` | `Result<u64, sqlx::Error>` | Chunked bulk insert from any `IntoIterator` |

`:execlastid` differs by engine because the databases do. PostgreSQL has no
last-insert-id, so sqlc requires a `RETURNING` clause and the value comes back
typed as that column. MySQL reports it on the query result, so the generated
function returns `u64` from `last_insert_id()` and the query needs no
`RETURNING`.

The batch annotations are PostgreSQL-only; sqlc does not accept them for MySQL.

All functions are free `pub async fn` (or `pub fn` for batch streams) at module scope, taking the executor as their first argument. The bound is `E: AsExecutor`, where `AsExecutor` is the trait emitted in each generated file.

Batch methods generate `Stream`-returning APIs and reference `futures_core` and `futures_util` directly. Consumer crates should include those dependencies alongside `sqlx`.

## sqlc extensions

- **`sqlc.slice()`**: Parameters marked as slice expand to `Vec<T>` and support runtime placeholder expansion for `IN (sqlc.slice(...))`-style queries. On PostgreSQL a query that binds the slice natively (`= ANY($1)`) skips the rewrite and passes the `Vec` straight through; every other case — and every MySQL query — expands to one placeholder per element, with an empty slice becoming `IN (NULL)`.
- **`sqlc.embed(table)`**: Result columns from an embedded table become a nested struct with `#[sqlx(flatten)]`.

## Contributing

We review change proposals in Discussions before code. [Start a Discussion](https://github.com/mathematic-inc/sqlc-gen-sqlx/discussions/new) and wait for a maintainer's review. If we accept the proposal, a Mathematic maintainer or agent will implement it and open the pull request. When Mathematic implements a proposal, the implementation PR will link to the Discussion and credit its original author. GitHub limits pull request creation to Mathematic maintainers and repository collaborators with write, maintain, or admin access, plus authorized maintenance agents. See [CONTRIBUTING.md](./CONTRIBUTING.md) for the full process.

## License

MIT OR Apache-2.0
