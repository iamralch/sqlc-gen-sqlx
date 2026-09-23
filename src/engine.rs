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
}

impl Engine {
    /// Resolve the engine from sqlc's `settings.engine`. An empty string keeps
    /// the historical PostgreSQL default so requests that omit settings (older
    /// sqlc releases, hand-built test requests) keep working.
    pub fn from_name(name: &str) -> Result<Self, Error> {
        match name.trim().to_ascii_lowercase().as_str() {
            "" | "postgresql" | "postgres" => Ok(Self::Postgresql),
            "mysql" => Ok(Self::Mysql),
            other => Err(Error::Codegen(format!(
                "unsupported sqlc engine '{other}'; sqlc-gen-sqlx supports 'postgresql' and 'mysql'"
            ))),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Postgresql => "postgresql",
            Self::Mysql => "mysql",
        }
    }

    /// The `sqlx::Database` implementor, e.g. `sqlx::Postgres`.
    pub fn database_type(self) -> TokenStream {
        match self {
            Self::Postgresql => quote! { sqlx::Postgres },
            Self::Mysql => quote! { sqlx::MySql },
        }
    }

    /// The driver's pool alias, e.g. `sqlx::PgPool`.
    pub fn pool_type(self) -> TokenStream {
        match self {
            Self::Postgresql => quote! { sqlx::PgPool },
            Self::Mysql => quote! { sqlx::MySqlPool },
        }
    }

    /// The driver's connection type, e.g. `sqlx::PgConnection`.
    pub fn connection_type(self) -> TokenStream {
        match self {
            Self::Postgresql => quote! { sqlx::PgConnection },
            Self::Mysql => quote! { sqlx::MySqlConnection },
        }
    }

    /// The `execute` return type, e.g. `sqlx::postgres::PgQueryResult`.
    pub fn query_result_type(self) -> TokenStream {
        match self {
            Self::Postgresql => quote! { sqlx::postgres::PgQueryResult },
            Self::Mysql => quote! { sqlx::mysql::MySqlQueryResult },
        }
    }

    pub fn placeholders(self) -> Placeholders {
        match self {
            Self::Postgresql => Placeholders::Numbered,
            Self::Mysql => Placeholders::Ordinal,
        }
    }

    pub fn last_insert_id(self) -> LastInsertId {
        match self {
            Self::Postgresql => LastInsertId::ReturningColumn,
            Self::Mysql => LastInsertId::MySqlLastInsertId,
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

    /// Upper bound on bind parameters in a single statement. `:copyfrom`
    /// chunks its multi-row INSERT to stay under this.
    pub fn max_bind_params(self) -> usize {
        match self {
            // Both wire protocols carry the parameter count as u16.
            Self::Postgresql | Self::Mysql => 65_535,
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
    }

    #[test]
    fn only_postgres_binds_arrays_natively() {
        assert!(Engine::Postgresql.supports_array_binding());
        assert!(!Engine::Mysql.supports_array_binding());
    }
}
