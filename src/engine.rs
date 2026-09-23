//! The database engine the plugin generates for.
//!
//! sqlc reports the engine in `GenerateRequest.settings.engine`. Everything
//! that differs between backends — the sqlx driver types, placeholder syntax,
//! bind-parameter limits, `:execlastid` semantics — hangs off [`Engine`] so the
//! codegen modules stay engine-agnostic.

use proc_macro2::TokenStream;
use quote::quote;

use crate::error::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Engine {
    #[default]
    Postgresql,
    Mysql,
    Sqlite,
}

/// How the engine spells bind parameters in SQL text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Placeholders {
    /// `$1`, `$2`, … — positions are explicit, so expanding a slice requires
    /// renumbering every placeholder that follows it.
    Numbered,
    /// `?` — positions are implicit and ordinal, so a slice expands in place
    /// and nothing downstream needs rewriting.
    Ordinal,
}

/// How `:execlastid` obtains the generated key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LastInsertId {
    /// PostgreSQL has no last-insert-id; sqlc requires `RETURNING`, so the
    /// value comes back as a result column.
    ReturningColumn,
    /// `MySqlQueryResult::last_insert_id() -> u64`.
    MySqlLastInsertId,
    /// `SqliteQueryResult::last_insert_rowid() -> i64`.
    SqliteLastInsertRowid,
}

/// How generated enums reach the wire, which decides what sqlx machinery the
/// codegen can lean on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnumRepr {
    /// A catalog type the protocol resolves by name (PostgreSQL). sqlx's `Type`
    /// derive handles it once given `#[sqlx(type_name)]`.
    NamedType,
    /// Text on the wire, and sqlx's `Type` derive already defers compatibility
    /// to `str` (SQLite).
    Text,
    /// Text on the wire, but sqlx's `Type` derive reports a type info the
    /// driver rejects for real `ENUM` columns (MySQL), so the `Type` impl has
    /// to be written out.
    TextManualType,
}

impl Engine {
    /// Resolve the engine from sqlc's `settings.engine`. An empty string keeps
    /// the historical PostgreSQL default so requests that omit settings (older
    /// sqlc releases, hand-built test requests) keep working.
    pub fn from_name(name: &str) -> Result<Self, Error> {
        match name.trim().to_ascii_lowercase().as_str() {
            "" | "postgresql" | "postgres" => Ok(Self::Postgresql),
            "mysql" => Ok(Self::Mysql),
            "sqlite" => Ok(Self::Sqlite),
            other => Err(Error::Codegen(format!(
                "unsupported sqlc engine '{other}'; \
                 sqlc-gen-sqlx supports 'postgresql', 'mysql' and 'sqlite'"
            ))),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Postgresql => "postgresql",
            Self::Mysql => "mysql",
            Self::Sqlite => "sqlite",
        }
    }

    /// The `sqlx::Database` implementor, e.g. `sqlx::Postgres`.
    pub fn database_type(self) -> TokenStream {
        match self {
            Self::Postgresql => quote! { sqlx::Postgres },
            Self::Mysql => quote! { sqlx::MySql },
            Self::Sqlite => quote! { sqlx::Sqlite },
        }
    }

    /// The driver's pool alias, e.g. `sqlx::PgPool`.
    pub fn pool_type(self) -> TokenStream {
        match self {
            Self::Postgresql => quote! { sqlx::PgPool },
            Self::Mysql => quote! { sqlx::MySqlPool },
            Self::Sqlite => quote! { sqlx::SqlitePool },
        }
    }

    /// The driver's connection type, e.g. `sqlx::PgConnection`.
    pub fn connection_type(self) -> TokenStream {
        match self {
            Self::Postgresql => quote! { sqlx::PgConnection },
            Self::Mysql => quote! { sqlx::MySqlConnection },
            Self::Sqlite => quote! { sqlx::SqliteConnection },
        }
    }

    /// The `execute` return type, e.g. `sqlx::postgres::PgQueryResult`.
    pub fn query_result_type(self) -> TokenStream {
        match self {
            Self::Postgresql => quote! { sqlx::postgres::PgQueryResult },
            Self::Mysql => quote! { sqlx::mysql::MySqlQueryResult },
            Self::Sqlite => quote! { sqlx::sqlite::SqliteQueryResult },
        }
    }

    pub fn placeholders(self) -> Placeholders {
        match self {
            Self::Postgresql => Placeholders::Numbered,
            Self::Mysql | Self::Sqlite => Placeholders::Ordinal,
        }
    }

    pub fn last_insert_id(self) -> LastInsertId {
        match self {
            Self::Postgresql => LastInsertId::ReturningColumn,
            Self::Mysql => LastInsertId::MySqlLastInsertId,
            Self::Sqlite => LastInsertId::SqliteLastInsertRowid,
        }
    }

    /// Whether the engine can bind a Rust slice to a single placeholder
    /// (PostgreSQL `= ANY($1)`). Engines without it must always expand
    /// `sqlc.slice()` into one placeholder per element.
    pub fn supports_array_binding(self) -> bool {
        matches!(self, Self::Postgresql)
    }

    /// Whether the engine has user-defined composite types.
    pub fn supports_composite_types(self) -> bool {
        matches!(self, Self::Postgresql)
    }

    /// How generated enums are represented on the wire.
    pub fn enum_repr(self) -> EnumRepr {
        match self {
            Self::Postgresql => EnumRepr::NamedType,
            Self::Mysql => EnumRepr::TextManualType,
            Self::Sqlite => EnumRepr::Text,
        }
    }

    /// Upper bound on bind parameters in a single statement. `:copyfrom`
    /// chunks its multi-row INSERT to stay under this.
    pub fn max_bind_params(self) -> usize {
        match self {
            // Both wire protocols carry the parameter count as u16.
            Self::Postgresql | Self::Mysql => 65_535,
            // SQLITE_MAX_VARIABLE_NUMBER, the default since SQLite 3.32.
            Self::Sqlite => 32_766,
        }
    }

    /// Render the placeholder for 1-based position `position`.
    pub fn placeholder(self, position: usize) -> String {
        match self.placeholders() {
            Placeholders::Numbered => format!("${position}"),
            Placeholders::Ordinal => "?".to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_engine_defaults_to_postgres() {
        assert_eq!(Engine::from_name("").unwrap(), Engine::Postgresql);
    }

    #[test]
    fn parses_known_engines() {
        assert_eq!(Engine::from_name("postgresql").unwrap(), Engine::Postgresql);
        assert_eq!(Engine::from_name("postgres").unwrap(), Engine::Postgresql);
        assert_eq!(Engine::from_name("mysql").unwrap(), Engine::Mysql);
        assert_eq!(Engine::from_name("sqlite").unwrap(), Engine::Sqlite);
    }

    #[test]
    fn parsing_is_case_and_space_insensitive() {
        assert_eq!(Engine::from_name("  MySQL ").unwrap(), Engine::Mysql);
    }

    #[test]
    fn rejects_unknown_engine() {
        let err = Engine::from_name("oracle").unwrap_err().to_string();
        assert!(err.contains("oracle"), "expected engine name in: {err}");
    }

    #[test]
    fn placeholders_match_engine() {
        assert_eq!(Engine::Postgresql.placeholder(3), "$3");
        assert_eq!(Engine::Mysql.placeholder(3), "?");
        assert_eq!(Engine::Sqlite.placeholder(3), "?");
    }

    #[test]
    fn sqlite_caps_bind_params_below_the_server_engines() {
        assert_eq!(Engine::Sqlite.max_bind_params(), 32_766);
        assert!(Engine::Sqlite.max_bind_params() < Engine::Postgresql.max_bind_params());
    }

    #[test]
    fn only_postgres_binds_arrays_natively() {
        assert!(Engine::Postgresql.supports_array_binding());
        assert!(!Engine::Mysql.supports_array_binding());
        assert!(!Engine::Sqlite.supports_array_binding());
    }

    #[test]
    fn only_postgres_derives_enums_by_type_name() {
        assert_eq!(Engine::Postgresql.enum_repr(), EnumRepr::NamedType);
        assert_eq!(Engine::Mysql.enum_repr(), EnumRepr::TextManualType);
        assert_eq!(Engine::Sqlite.enum_repr(), EnumRepr::Text);
    }
}
