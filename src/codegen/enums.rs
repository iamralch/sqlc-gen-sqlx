// src/codegen/enums.rs
use proc_macro2::TokenStream;
use quote::quote;
use syn::parse_str;

use crate::{
    catalog::EnumInfo,
    engine::{Engine, EnumRepr},
    error::Error,
    ident::{type_ident, variant_ident},
};

/// Emit a Rust enum from a database ENUM type.
///
/// PostgreSQL output:
/// ```text
/// #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, sqlx::Type)]
/// #[sqlx(type_name = "status")]
/// pub enum Status {
///     #[sqlx(rename = "active")]
///     Active,
/// }
/// ```
///
/// Engines that keep enums as text drop `type_name`, since there is no catalog
/// type to resolve by name. MySQL additionally cannot use sqlx's `Type` derive:
/// its MySQL impl reports `MySqlTypeInfo::__enum()`, which the driver's
/// compatibility check does not accept for a real `ENUM` column, so decoding
/// one fails at runtime with "mismatched types". There, only `Encode`/`Decode`
/// are derived and the `Type` impl is written out, deferring to `str` — which
/// accepts `ENUM`, `CHAR`, `VARCHAR` and the `TEXT` family. sqlx's SQLite impl
/// already defers to `str`, so the derive is enough.
pub fn gen_enum(
    info: &EnumInfo,
    engine: Engine,
    extra_derives: &[String],
) -> Result<TokenStream, Error> {
    let rust_name = type_ident(&info.rust_name);
    let type_name = &info.type_name;

    let mut variant_tokens = Vec::new();
    for val in &info.vals {
        let variant_name = variant_ident(val); // PascalCase, keyword-safe
        let rename = val.as_str();
        variant_tokens.push(quote! {
            #[sqlx(rename = #rename)]
            #variant_name,
        });
    }

    let mut derive_paths = Vec::new();
    for d in extra_derives {
        let path: syn::Path =
            parse_str(d).map_err(|e| Error::Codegen(format!("invalid derive path '{d}': {e}")))?;
        derive_paths.push(quote! { #path });
    }

    Ok(match engine.enum_repr() {
        EnumRepr::NamedType => quote! {
            #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, sqlx::Type, #(#derive_paths),*)]
            #[sqlx(type_name = #type_name)]
            pub enum #rust_name {
                #(#variant_tokens)*
            }
        },
        EnumRepr::Text => quote! {
            #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, sqlx::Type, #(#derive_paths),*)]
            pub enum #rust_name {
                #(#variant_tokens)*
            }
        },
        EnumRepr::TextManualType => quote! {
            #[derive(
                Debug, Clone, Copy, PartialEq, Eq, Hash, sqlx::Encode, sqlx::Decode,
                #(#derive_paths),*
            )]
            pub enum #rust_name {
                #(#variant_tokens)*
            }

            impl sqlx::Type<sqlx::MySql> for #rust_name {
                fn type_info() -> sqlx::mysql::MySqlTypeInfo {
                    <str as sqlx::Type<sqlx::MySql>>::type_info()
                }

                fn compatible(ty: &sqlx::mysql::MySqlTypeInfo) -> bool {
                    <str as sqlx::Type<sqlx::MySql>>::compatible(ty)
                }
            }
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::EnumInfo;

    fn status_enum() -> EnumInfo {
        EnumInfo {
            schema: "public".to_string(),
            db_name: "status".to_string(),
            rust_name: "Status".to_string(),
            type_name: "status".to_string(),
            vals: vec![
                "active".to_string(),
                "inactive".to_string(),
                "banned".to_string(),
            ],
        }
    }

    #[test]
    fn generates_enum_with_variants() {
        let tokens = gen_enum(&status_enum(), Engine::Postgresql, &[]).unwrap();
        let code = tokens.to_string();
        assert!(code.contains("Status"), "expected 'Status' in:\n{code}");
        assert!(code.contains("Active"), "expected 'Active' in:\n{code}");
        assert!(code.contains("Inactive"), "expected 'Inactive' in:\n{code}");
        assert!(code.contains("Banned"), "expected 'Banned' in:\n{code}");
    }

    #[test]
    fn generates_sqlx_rename_attrs() {
        let tokens = gen_enum(&status_enum(), Engine::Postgresql, &[]).unwrap();
        let code = tokens.to_string();
        assert!(
            code.contains(r#""active""#),
            "expected rename = \"active\" in:\n{code}"
        );
        assert!(
            code.contains(r#""inactive""#),
            "expected rename = \"inactive\" in:\n{code}"
        );
    }

    #[test]
    fn generates_sqlx_type_name_attr() {
        let tokens = gen_enum(&status_enum(), Engine::Postgresql, &[]).unwrap();
        let code = tokens.to_string();
        assert!(
            code.contains(r#""status""#),
            "expected type_name = \"status\" in:\n{code}"
        );
    }

    #[test]
    fn appends_extra_derives() {
        let tokens = gen_enum(
            &status_enum(),
            Engine::Postgresql,
            &["serde::Serialize".to_string()],
        )
        .unwrap();
        let code = tokens.to_string();
        // quote serializes :: as " :: " with spaces
        assert!(
            code.contains("serde :: Serialize") || code.contains("serde::Serialize"),
            "expected serde::Serialize in:\n{code}"
        );
    }

    #[test]
    fn mysql_enum_defers_compatibility_to_str() {
        let code = gen_enum(&status_enum(), Engine::Mysql, &[])
            .unwrap()
            .to_string();
        assert!(
            !code.contains("type_name"),
            "MySQL enums must not carry #[sqlx(type_name)] in:\n{code}"
        );
        assert!(
            !code.contains("sqlx :: Type ,") && !code.contains("sqlx :: Type,"),
            "MySQL enums must not derive sqlx::Type in:\n{code}"
        );
        for expected in ["sqlx :: Encode", "sqlx :: Decode", "fn compatible"] {
            assert!(code.contains(expected), "expected {expected} in:\n{code}");
        }
    }

    #[test]
    fn mysql_enum_keeps_variant_renames() {
        let code = gen_enum(&status_enum(), Engine::Mysql, &[])
            .unwrap()
            .to_string();
        assert!(
            code.contains(r#""active""#),
            "expected rename = \"active\" in:\n{code}"
        );
    }

    #[test]
    fn sqlite_enum_uses_the_derive_without_a_type_name() {
        let code = gen_enum(&status_enum(), Engine::Sqlite, &[])
            .unwrap()
            .to_string();
        // sqlx's SQLite `Type` impl already defers compatibility to `str`, so
        // unlike MySQL the derive is enough on its own.
        assert!(
            code.contains("sqlx :: Type"),
            "expected the sqlx::Type derive in:\n{code}"
        );
        assert!(
            !code.contains("type_name"),
            "SQLite enums must not carry #[sqlx(type_name)] in:\n{code}"
        );
        assert!(
            !code.contains("impl sqlx"),
            "SQLite needs no hand-written impl in:\n{code}"
        );
    }

    #[test]
    fn invalid_extra_derive_returns_error() {
        let result = gen_enum(
            &status_enum(),
            Engine::Postgresql,
            &["not a path !!!".to_string()],
        );
        assert!(result.is_err(), "expected error for invalid derive path");
    }
}
