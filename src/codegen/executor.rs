//! The `AsExecutor` trait emitted at the top of every generated file.
//!
//! Generated query functions take `impl AsExecutor` rather than sqlx's
//! `Executor` directly so callers can pass a pool, a connection, or a
//! transaction by reference without naming lifetimes. The set of impls is the
//! same shape for every backend — only the driver types change.

use proc_macro2::TokenStream;
use quote::quote;

use crate::engine::Engine;

pub fn gen_as_executor(engine: Engine) -> TokenStream {
    let db = engine.database_type();
    let pool = engine.pool_type();
    let conn = engine.connection_type();

    quote! {
        pub trait AsExecutor {
            fn as_executor(&mut self) -> impl sqlx::Executor<'_, Database = #db>;
        }

        impl AsExecutor for #pool {
            fn as_executor(&mut self) -> impl sqlx::Executor<'_, Database = #db> {
                &*self
            }
        }

        impl AsExecutor for &#pool {
            fn as_executor(&mut self) -> impl sqlx::Executor<'_, Database = #db> {
                *self
            }
        }

        impl AsExecutor for #conn {
            fn as_executor(&mut self) -> impl sqlx::Executor<'_, Database = #db> {
                &mut *self
            }
        }

        impl AsExecutor for sqlx::Transaction<'_, #db> {
            fn as_executor(&mut self) -> impl sqlx::Executor<'_, Database = #db> {
                &mut **self
            }
        }

        impl AsExecutor for sqlx::pool::PoolConnection<#db> {
            fn as_executor(&mut self) -> impl sqlx::Executor<'_, Database = #db> {
                &mut **self
            }
        }

        impl<T: AsExecutor + ?Sized> AsExecutor for &mut T {
            fn as_executor(&mut self) -> impl sqlx::Executor<'_, Database = #db> {
                (**self).as_executor()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn postgres_impls_reference_pg_types() {
        let code = gen_as_executor(Engine::Postgresql).to_string();
        assert!(code.contains("PgPool"), "expected PgPool in:\n{code}");
        assert!(code.contains("Postgres"), "expected Postgres in:\n{code}");
        assert!(!code.contains("MySql"), "unexpected MySql in:\n{code}");
    }

    #[test]
    fn sqlite_impls_reference_sqlite_types() {
        let code = gen_as_executor(Engine::Sqlite).to_string();
        assert!(
            code.contains("SqlitePool"),
            "expected SqlitePool in:\n{code}"
        );
        assert!(!code.contains("MySql"), "unexpected MySql in:\n{code}");
    }

    #[test]
    fn mysql_impls_reference_mysql_types() {
        let code = gen_as_executor(Engine::Mysql).to_string();
        assert!(code.contains("MySqlPool"), "expected MySqlPool in:\n{code}");
        assert!(
            code.contains("MySqlConnection"),
            "expected MySqlConnection in:\n{code}"
        );
        assert!(!code.contains("PgPool"), "unexpected PgPool in:\n{code}");
    }
}
