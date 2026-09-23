use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use syn::parse_str;

use crate::{
    codegen::{Ctx, lifetimes::inject_lifetime},
    engine::{LastInsertId, Placeholders},
    error::Error,
    ident::{field_ident, query_params_name, to_pascal_case, to_snake_case, type_ident},
    plugin::{ColumnView, ParameterView, QueryView},
    types::ResolvedType,
};

/// Resolved parameter: Rust identifier + type.
pub(crate) struct Param {
    pub(crate) number: i32,
    pub(crate) ident: proc_macro2::Ident,
    pub(crate) source_name: String,
    pub(crate) is_slice: bool,
    /// For `sqlc.slice()` parameters, the name sqlc used in the
    /// `/*SLICE:name*/` marker it left in the query text. This is the column's
    /// `name`, which is *not* always `source_name`: `WHERE id IN
    /// (sqlc.slice(ids))` reports `name = "ids"` but `original_name = "id"`,
    /// and the marker follows `name`.
    pub(crate) slice_name: Option<String>,
    pub(crate) resolved: ResolvedType,
}

impl Param {
    /// Parameter-position type: borrowed form if the resolver produced one,
    /// otherwise the owned form. Lifetimes are anonymous (`&str`).
    pub(crate) fn param_type(&self) -> &str {
        self.resolved
            .borrowed_rust_type
            .as_deref()
            .unwrap_or(&self.resolved.rust_type)
    }

    pub(crate) fn is_borrowed(&self) -> bool {
        self.resolved.borrowed_rust_type.is_some()
    }
}

/// Columns from an embedded table (`sqlc.embed(table)`), grouped together.
pub(crate) struct EmbeddedGroup {
    /// snake_case field name in the parent struct, e.g. `author`
    pub(crate) embed_field_ident: proc_macro2::Ident,
    /// PascalCase struct name for the sub-struct, e.g. `AuthorEmbed`
    pub(crate) struct_ident: proc_macro2::Ident,
    /// Fields of the sub-struct in declaration order
    pub(crate) fields: Vec<(proc_macro2::Ident, ResolvedType)>,
}

/// Result of resolving a query's output columns.
pub(crate) struct ResolvedColumnSet {
    /// Columns that map directly to fields in the row struct.
    pub(crate) flat: Vec<(proc_macro2::Ident, ResolvedType)>,
    /// Groups of columns that map to embedded sub-structs (from `sqlc.embed()`).
    pub(crate) embedded: Vec<EmbeddedGroup>,
}

pub(crate) fn resolve_params<'a>(
    params: impl Iterator<Item = &'a ParameterView<'a>>,
    ctx: &Ctx<'_>,
) -> Result<Vec<Param>, Error> {
    let mut out = Vec::new();
    for p in params {
        let col: &ColumnView<'_> = p
            .column
            .as_option()
            .ok_or_else(|| Error::Codegen("parameter missing column".into()))?;
        let db_type = col.r#type.as_option().map(|t| t.name).unwrap_or("");
        let nullable = !col.not_null;
        let array_dims = if col.is_sqlc_slice {
            1usize
        } else if col.array_dims > 0 {
            col.array_dims as usize
        } else {
            usize::from(col.is_array)
        };
        let col_key = col
            .table
            .as_option()
            .map(|t| format!("{}.{}", t.name, col.name));
        let resolved = ctx
            .type_map
            .resolve_column_dims(
                db_type,
                nullable,
                array_dims,
                col_key.as_deref(),
                ctx.col_overrides,
            )
            .ok_or_else(|| unknown_type_error(ctx, db_type))?;
        let param_name = if col.is_named_param && !col.original_name.is_empty() {
            col.original_name
        } else {
            col.name
        };
        out.push(Param {
            number: p.number,
            ident: field_ident(param_name),
            source_name: param_name.to_string(),
            is_slice: col.is_sqlc_slice,
            slice_name: col.is_sqlc_slice.then(|| col.name.to_string()),
            resolved,
        });
    }
    Ok(out)
}

fn unknown_type_error(ctx: &Ctx<'_>, db_type: &str) -> Error {
    Error::Codegen(format!(
        "unknown {} type: {db_type}",
        ctx.engine.as_str().to_uppercase()
    ))
}

/// Whether any param in the set carries a borrowed type.
pub(crate) fn any_borrowed(params: &[Param]) -> bool {
    params.iter().any(Param::is_borrowed)
}

/// Emit a params struct when the query has ≥2 parameters. When any field is
/// borrowed, the struct gains a `<'a>` lifetime parameter and each borrowed
/// field references it.
pub(crate) fn maybe_params_struct(
    query_name: &str,
    params: &[Param],
    extra_derives: &[String],
) -> Result<Option<(TokenStream, proc_macro2::Ident)>, Error> {
    if params.len() < 2 {
        return Ok(None);
    }
    let struct_name = type_ident(&query_params_name(query_name));
    let has_borrowed = any_borrowed(params);
    let mut field_tokens = Vec::new();
    for p in params {
        let ident = &p.ident;
        let ty_str = if let Some(borrowed) = &p.resolved.borrowed_rust_type {
            inject_lifetime(borrowed, "'a")?
        } else {
            p.resolved.rust_type.clone()
        };
        let ty: syn::Type = parse_str(&ty_str)
            .map_err(|e| Error::Codegen(format!("invalid Rust type '{ty_str}': {e}")))?;
        field_tokens.push(quote! { pub #ident: #ty, });
    }
    let mut derive_paths = Vec::new();
    for d in extra_derives {
        let path: syn::Path =
            parse_str(d).map_err(|e| Error::Codegen(format!("invalid derive path '{d}': {e}")))?;
        derive_paths.push(quote! { #path });
    }
    let generics = if has_borrowed {
        quote! { <'a> }
    } else {
        quote! {}
    };
    let tokens = quote! {
        #[derive(Debug, Clone, #(#derive_paths),*)]
        pub struct #struct_name #generics {
            #(#field_tokens)*
        }
    };
    Ok(Some((tokens, struct_name)))
}

/// Emit `.bind(...)` calls for a query function body.
pub(crate) fn bind_calls(params: &[Param], use_arg: Option<&proc_macro2::Ident>) -> TokenStream {
    params
        .iter()
        .map(|p| {
            let ident = &p.ident;
            let value = if let Some(arg) = use_arg {
                quote! { #arg.#ident }
            } else {
                quote! { #ident }
            };
            quote! { .bind(#value) }
        })
        .collect()
}

fn param_value_expr(param: &Param, use_arg: Option<&proc_macro2::Ident>) -> TokenStream {
    let ident = &param.ident;
    if let Some(arg) = use_arg {
        quote! { #arg.#ident }
    } else {
        quote! { #ident }
    }
}

fn ordered_params(params: &[Param]) -> Vec<&Param> {
    let mut ordered = params.iter().collect::<Vec<_>>();
    ordered.sort_by_key(|p| p.number);
    ordered
}

fn reverse_non_slice_params(params: &[Param]) -> Vec<&Param> {
    let mut ordered = params.iter().filter(|p| !p.is_slice).collect::<Vec<_>>();
    ordered.sort_by_key(|p| std::cmp::Reverse(p.number));
    ordered
}

/// Whether the query needs its SQL rewritten at call time because a
/// `sqlc.slice()` parameter expands to one placeholder per element.
///
/// PostgreSQL can skip the rewrite when the query binds the slice natively
/// (`= ANY($1)`). Engines without array binding always rewrite.
pub(crate) fn has_dynamic_slice(ctx: &Ctx<'_>, sql: &str, params: &[Param]) -> bool {
    params.iter().any(|param| {
        param.is_slice
            && !(ctx.engine.supports_array_binding()
                && uses_native_array_binding(sql, param.number))
    })
}

fn uses_native_array_binding(sql: &str, param_number: i32) -> bool {
    let compact = sql
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect::<String>();
    let placeholder = format!("${param_number}");

    compact.contains(&format!("ANY({placeholder})"))
        || compact.contains(&format!("ANY({placeholder}::"))
        || compact.contains(&format!("ALL({placeholder})"))
        || compact.contains(&format!("ALL({placeholder}::"))
}

/// The two halves of a query whose SQL is rewritten at call time: the statements
/// that build the final SQL string, and the `.bind()` calls that match it.
///
/// They are produced together because the bind order depends on the same
/// placeholder analysis the rewrite does.
pub(crate) struct DynamicQuery {
    pub(crate) setup: TokenStream,
    pub(crate) binds: TokenStream,
}

pub(crate) fn dynamic_query(
    ctx: &Ctx<'_>,
    sql: &str,
    sql_const: &proc_macro2::Ident,
    params: &[Param],
    use_arg: Option<&proc_macro2::Ident>,
) -> Result<DynamicQuery, Error> {
    Ok(match ctx.engine.placeholders() {
        Placeholders::Numbered => DynamicQuery {
            setup: numbered_sql_setup(sql_const, params, use_arg),
            binds: bind_statements(&ordered_params(params), use_arg),
        },
        Placeholders::Ordinal => {
            let order = ordinal_bind_order(sql, params)?;
            DynamicQuery {
                setup: ordinal_sql_setup(sql_const, &order, use_arg),
                binds: bind_statements(&order, use_arg),
            }
        }
    })
}

/// A bind position found in the query text.
enum Slot {
    /// A bare `?`.
    Scalar,
    /// A `/*SLICE:name*/?` marker left by `sqlc.slice()`.
    Slice(String),
}

/// Find the bind positions of a `?`-style query, left to right.
///
/// Quoted strings are skipped so a literal `?` or `/*` inside them is not
/// mistaken for a placeholder.
fn scan_ordinal_slots(sql: &str) -> Vec<Slot> {
    const MARKER: &str = "/*SLICE:";
    let bytes = sql.as_bytes();
    let mut slots = Vec::new();
    let mut i = 0;

    while i < bytes.len() {
        match bytes[i] {
            quote @ (b'\'' | b'"' | b'`') => {
                i += 1;
                while i < bytes.len() {
                    if bytes[i] == b'\\' {
                        i += 2;
                    } else if bytes[i] == quote {
                        i += 1;
                        break;
                    } else {
                        i += 1;
                    }
                }
            }
            b'/' if sql[i..].starts_with(MARKER) => {
                let rest = &sql[i + MARKER.len()..];
                let Some(end) = rest.find("*/") else {
                    i += 1;
                    continue;
                };
                slots.push(Slot::Slice(rest[..end].to_string()));
                i += MARKER.len() + end + "*/".len();
                // sqlc always emits the placeholder right after the marker.
                if bytes.get(i) == Some(&b'?') {
                    i += 1;
                }
            }
            b'?' => {
                slots.push(Slot::Scalar);
                i += 1;
            }
            _ => i += 1,
        }
    }

    slots
}

/// Order the parameters the way a `?` engine will bind them: by position in the
/// query text.
///
/// This is not the same as ordering by `Parameter.number`. sqlc lifts
/// `sqlc.slice()` out of the ordinal numbering, so `WHERE id IN
/// (sqlc.slice(ids)) AND country = ?` reports `country` as number 1 and `ids`
/// as number 2 even though the slice comes first in the text.
fn ordinal_bind_order<'a>(sql: &str, params: &'a [Param]) -> Result<Vec<&'a Param>, Error> {
    let mut scalars = params.iter().filter(|p| !p.is_slice).collect::<Vec<_>>();
    scalars.sort_by_key(|p| p.number);
    let mut scalars = scalars.into_iter();

    let mut order = Vec::new();
    for slot in scan_ordinal_slots(sql) {
        let param = match slot {
            Slot::Scalar => scalars.next().ok_or_else(|| {
                Error::Codegen(format!(
                    "query has more '?' placeholders than parameters: {sql}"
                ))
            })?,
            Slot::Slice(name) => params
                .iter()
                .find(|p| p.slice_name.as_deref() == Some(name.as_str()))
                .ok_or_else(|| {
                    Error::Codegen(format!(
                        "query references sqlc.slice('{name}') but no parameter reports that name"
                    ))
                })?,
        };
        order.push(param);
    }

    if scalars.next().is_some() {
        return Err(Error::Codegen(format!(
            "query has more parameters than '?' placeholders: {sql}"
        )));
    }

    Ok(order)
}

/// Slice expansion for `?` engines.
///
/// Placeholder positions are implicit, so a slice expands in place and no other
/// placeholder needs rewriting — the whole rewrite is one `replace` per slice.
fn ordinal_sql_setup(
    sql_const: &proc_macro2::Ident,
    bind_order: &[&Param],
    use_arg: Option<&proc_macro2::Ident>,
) -> TokenStream {
    let mut tokens = vec![quote! {
        let mut sql = #sql_const.to_string();
    }];

    for param in bind_order.iter().filter(|p| p.is_slice) {
        let value_expr = param_value_expr(param, use_arg);
        // SAFETY: `is_slice` implies `slice_name` was populated.
        let marker = format!(
            "/*SLICE:{}*/?",
            param.slice_name.as_deref().unwrap_or(&param.source_name)
        );
        tokens.push(quote! {
            {
                let slice_len = (#value_expr).len();
                let replacement = if slice_len == 0 {
                    "NULL".to_string()
                } else {
                    std::iter::repeat_n("?", slice_len).collect::<Vec<_>>().join(", ")
                };
                sql = sql.replace(#marker, &replacement);
            }
        });
    }

    quote! { #(#tokens)* }
}

/// Slice expansion for `$N` engines.
///
/// Expanding a slice shifts every placeholder after it, so non-slice
/// placeholders are first parked under unique sentinels (highest number first,
/// so `$1` cannot corrupt `$10`) and then renumbered into their final
/// positions.
fn numbered_sql_setup(
    sql_const: &proc_macro2::Ident,
    params: &[Param],
    use_arg: Option<&proc_macro2::Ident>,
) -> TokenStream {
    let mut tokens = Vec::new();
    tokens.push(quote! {
        let mut sql = #sql_const.to_string();
    });

    for param in reverse_non_slice_params(params) {
        let original = format!("${}", param.number);
        let temporary = format!("__SQLC_PARAM_{}__", param.number);
        tokens.push(quote! {
            sql = sql.replace(#original, #temporary);
        });
    }

    let ordered = ordered_params(params);
    if ordered.len() > 1 {
        tokens.push(quote! {
            let mut next_placeholder = 1usize;
        });
    } else if ordered.len() == 1 {
        tokens.push(quote! {
            let next_placeholder = 1usize;
        });
    }
    let last_idx = ordered.len().saturating_sub(1);
    for (idx, param) in ordered.iter().enumerate() {
        let is_last = idx == last_idx;
        let placeholder_ident = format_ident!("placeholder_{}", param.number as usize);
        tokens.push(quote! {
            let #placeholder_ident = next_placeholder;
        });

        if param.is_slice {
            let value_expr = param_value_expr(param, use_arg);
            let marker = format!("/*SLICE:{}*/?", param.source_name);
            let numbered_marker = format!("/*SLICE:{}*/${}", param.source_name, param.number);
            let bare_placeholder = format!("${}", param.number);
            let advance = if is_last {
                quote! {}
            } else {
                quote! { next_placeholder += slice_len; }
            };
            tokens.push(quote! {
                let slice_len = (#value_expr).len();
                let replacement = if slice_len == 0 {
                    "NULL".to_string()
                } else {
                    (#placeholder_ident..(#placeholder_ident + slice_len))
                        .map(|n| format!("${}", n))
                        .collect::<Vec<_>>()
                        .join(", ")
                };
                if sql.contains(#marker) {
                    sql = sql.replace(#marker, &replacement);
                } else {
                    if sql.contains(#numbered_marker) {
                        sql = sql.replace(#numbered_marker, &replacement);
                    } else {
                        sql = sql.replace(#bare_placeholder, &replacement);
                    }
                }
                #advance
            });
        } else {
            let temporary = format!("__SQLC_PARAM_{}__", param.number);
            let advance = if is_last {
                quote! {}
            } else {
                quote! { next_placeholder += 1; }
            };
            tokens.push(quote! {
                sql = sql.replace(#temporary, &format!("${}", #placeholder_ident));
                #advance
            });
        }
    }

    quote! { #(#tokens)* }
}

/// Emit `query = query.bind(..)` statements in the given order, expanding slice
/// parameters into one bind per element.
fn bind_statements(order: &[&Param], use_arg: Option<&proc_macro2::Ident>) -> TokenStream {
    order
        .iter()
        .map(|param| {
            let value_expr = param_value_expr(param, use_arg);
            if param.is_slice {
                quote! {
                    for value in #value_expr {
                        query = query.bind(value);
                    }
                }
            } else {
                quote! {
                    query = query.bind(#value_expr);
                }
            }
        })
        .collect()
}

/// Generate a SQL string constant, returning both the token stream and the
/// constant's identifier so callers don't need to recompute it.
pub(crate) fn sql_const(query_name: &str, sql: &str) -> (TokenStream, proc_macro2::Ident) {
    let const_name = format_ident!("{}", to_snake_case(query_name).to_uppercase());
    let tokens = quote! {
        pub const #const_name: &str = #sql;
    };
    (tokens, const_name)
}

/// Resolve result columns into flat fields and embedded groups. Row positions
/// always use the owned form, so any `borrowed_rust_type` on the resolution
/// is intentionally ignored downstream.
pub(crate) fn resolve_columns<'a>(
    cols: impl Iterator<Item = &'a ColumnView<'a>>,
    ctx: &Ctx<'_>,
) -> Result<ResolvedColumnSet, Error> {
    let mut flat: Vec<(proc_macro2::Ident, ResolvedType)> = Vec::new();
    let mut embedded_groups: Vec<(String, Vec<(proc_macro2::Ident, ResolvedType)>)> = Vec::new();

    for col in cols {
        let db_type = col.r#type.as_option().map(|t| t.name).unwrap_or("");
        if db_type.is_empty() {
            return Err(Error::Codegen(format!("column '{}' has no type", col.name)));
        }
        let nullable = !col.not_null;
        let array_dims = if col.array_dims > 0 {
            col.array_dims as usize
        } else {
            usize::from(col.is_array)
        };
        let col_key = col
            .table
            .as_option()
            .map(|t| format!("{}.{}", t.name, col.name));
        let resolved = ctx
            .type_map
            .resolve_column_dims(
                db_type,
                nullable,
                array_dims,
                col_key.as_deref(),
                ctx.col_overrides,
            )
            .ok_or_else(|| unknown_type_error(ctx, db_type))?;

        if let Some(embed_id) = col.embed_table.as_option() {
            let embed_name = embed_id.name.to_string();
            if let Some(group) = embedded_groups.iter_mut().find(|(n, _)| n == &embed_name) {
                group.1.push((field_ident(col.name), resolved));
            } else {
                embedded_groups.push((embed_name, vec![(field_ident(col.name), resolved)]));
            }
        } else {
            flat.push((field_ident(col.name), resolved));
        }
    }

    let embedded = embedded_groups
        .into_iter()
        .map(|(name, fields)| EmbeddedGroup {
            embed_field_ident: field_ident(&name),
            struct_ident: type_ident(&format!("{}Embed", to_pascal_case(&name))),
            fields,
        })
        .collect();

    Ok(ResolvedColumnSet { flat, embedded })
}

/// Emit the row struct for :one / :many, handling both flat and embedded columns.
pub(crate) fn row_struct(
    query_name: &str,
    cols: &ResolvedColumnSet,
    extra_derives: &[String],
) -> Result<TokenStream, Error> {
    let struct_name = type_ident(&crate::ident::query_row_name(query_name));

    let mut derive_paths: Vec<proc_macro2::TokenStream> = Vec::new();
    for d in extra_derives {
        let path: syn::Path =
            parse_str(d).map_err(|e| Error::Codegen(format!("invalid derive path '{d}': {e}")))?;
        derive_paths.push(quote! { #path });
    }

    let mut embed_struct_tokens: Vec<proc_macro2::TokenStream> = Vec::new();
    let mut field_tokens: Vec<proc_macro2::TokenStream> = Vec::new();

    for embed in &cols.embedded {
        let embed_struct_ident = &embed.struct_ident;
        let mut embed_field_tokens: Vec<proc_macro2::TokenStream> = Vec::new();
        for (ident, resolved) in &embed.fields {
            let ty: syn::Type = parse_str(&resolved.rust_type).map_err(|e| {
                Error::Codegen(format!("invalid Rust type '{}': {e}", resolved.rust_type))
            })?;
            embed_field_tokens.push(quote! { pub #ident: #ty, });
        }
        embed_struct_tokens.push(quote! {
            #[derive(Debug, Clone, sqlx::FromRow, #(#derive_paths),*)]
            pub struct #embed_struct_ident {
                #(#embed_field_tokens)*
            }
        });
        let f_ident = &embed.embed_field_ident;
        field_tokens.push(quote! {
            #[sqlx(flatten)]
            pub #f_ident: #embed_struct_ident,
        });
    }

    for (ident, resolved) in &cols.flat {
        let ty: syn::Type = parse_str(&resolved.rust_type).map_err(|e| {
            Error::Codegen(format!("invalid Rust type '{}': {e}", resolved.rust_type))
        })?;
        field_tokens.push(quote! { pub #ident: #ty, });
    }

    Ok(quote! {
        #(#embed_struct_tokens)*
        #[derive(Debug, Clone, sqlx::FromRow, #(#derive_paths),*)]
        pub struct #struct_name {
            #(#field_tokens)*
        }
    })
}

/// Build the parameters portion of a query function signature.
/// Returns `(params_struct_tokens, arg_ident, fn_params_tokens)`.
///
/// When the query has a params struct and any field is borrowed, the
/// fn-signature reference uses `<'_>` to elide the struct's `'a` (Rust 2024
/// anonymous lifetime in type paths). Single-param functions rely on
/// classical lifetime elision and take the borrowed type directly.
pub(crate) fn build_fn_params(
    query_name: &str,
    params: &[Param],
    derives: &[String],
) -> Result<(Option<TokenStream>, Option<proc_macro2::Ident>, TokenStream), Error> {
    if params.len() >= 2 {
        // SAFETY: `maybe_params_struct` returns `Some` when `params.len() >= 2`.
        let (struct_tokens, struct_ident) = maybe_params_struct(query_name, params, derives)?
            .expect("guarded by params.len() >= 2 check above");
        let arg = format_ident!("arg");
        let arg_ty = if any_borrowed(params) {
            quote! { #struct_ident<'_> }
        } else {
            quote! { #struct_ident }
        };
        Ok((
            Some(struct_tokens),
            Some(arg.clone()),
            quote! { #arg: #arg_ty },
        ))
    } else if params.len() == 1 {
        let p = &params[0];
        let ident = &p.ident;
        let ty_str = p.param_type();
        let ty: syn::Type = parse_str(ty_str)
            .map_err(|e| Error::Codegen(format!("invalid Rust type '{ty_str}': {e}")))?;
        Ok((None, None, quote! { #ident: #ty }))
    } else {
        Ok((None, None, quote! {}))
    }
}

/// `:one` → `async fn foo<E: AsExecutor>(mut db: E, [params]) -> Result<FooRow, sqlx::Error>`
pub fn gen_one(query: &QueryView<'_>, ctx: &Ctx<'_>) -> Result<TokenStream, Error> {
    let params = resolve_params(query.params.iter(), ctx)?;
    let columns = resolve_columns(query.columns.iter(), ctx)?;

    let fn_name = format_ident!("{}", to_snake_case(query.name));
    let row_name = type_ident(&crate::ident::query_row_name(query.name));
    let (const_tokens, const_name) = sql_const(query.name, query.text);

    let row_tokens = row_struct(query.name, &columns, ctx.row_derives())?;

    let (params_struct, arg_ident, fn_params) =
        build_fn_params(query.name, &params, ctx.row_derives())?;

    let binds = bind_calls(&params, arg_ident.as_ref());
    let dynamic_slice = has_dynamic_slice(ctx, query.text, &params);

    let fn_tokens = if dynamic_slice {
        let DynamicQuery {
            setup: sql_setup,
            binds: bind_setup,
        } = dynamic_query(ctx, query.text, &const_name, &params, arg_ident.as_ref())?;
        quote! {
            pub async fn #fn_name<E: AsExecutor>(mut db: E, #fn_params) -> Result<#row_name, sqlx::Error> {
                #sql_setup
                let mut query = sqlx::query_as::<_, #row_name>(sqlx::AssertSqlSafe(sql));
                #bind_setup
                query.fetch_one(db.as_executor()).await
            }
        }
    } else {
        quote! {
            pub async fn #fn_name<E: AsExecutor>(mut db: E, #fn_params) -> Result<#row_name, sqlx::Error> {
                sqlx::query_as::<_, #row_name>(#const_name)
                    #binds
                    .fetch_one(db.as_executor())
                    .await
            }
        }
    };

    Ok(quote! {
        #params_struct
        #const_tokens
        #row_tokens
        #fn_tokens
    })
}

/// `:many` → `async fn foo<E: AsExecutor>(mut db: E, [params]) -> Result<Vec<FooRow>, sqlx::Error>`
pub fn gen_many(query: &QueryView<'_>, ctx: &Ctx<'_>) -> Result<TokenStream, Error> {
    let params = resolve_params(query.params.iter(), ctx)?;
    let columns = resolve_columns(query.columns.iter(), ctx)?;

    let fn_name = format_ident!("{}", to_snake_case(query.name));
    let row_name = type_ident(&crate::ident::query_row_name(query.name));
    let (const_tokens, const_name) = sql_const(query.name, query.text);

    let row_tokens = row_struct(query.name, &columns, ctx.row_derives())?;
    let (params_struct, arg_ident, fn_params) =
        build_fn_params(query.name, &params, ctx.row_derives())?;
    let binds = bind_calls(&params, arg_ident.as_ref());
    let dynamic_slice = has_dynamic_slice(ctx, query.text, &params);

    let fn_tokens = if dynamic_slice {
        let DynamicQuery {
            setup: sql_setup,
            binds: bind_setup,
        } = dynamic_query(ctx, query.text, &const_name, &params, arg_ident.as_ref())?;
        quote! {
            pub async fn #fn_name<E: AsExecutor>(mut db: E, #fn_params) -> Result<Vec<#row_name>, sqlx::Error> {
                #sql_setup
                let mut query = sqlx::query_as::<_, #row_name>(sqlx::AssertSqlSafe(sql));
                #bind_setup
                query.fetch_all(db.as_executor()).await
            }
        }
    } else {
        quote! {
            pub async fn #fn_name<E: AsExecutor>(mut db: E, #fn_params) -> Result<Vec<#row_name>, sqlx::Error> {
                sqlx::query_as::<_, #row_name>(#const_name)
                    #binds
                    .fetch_all(db.as_executor())
                    .await
            }
        }
    };

    Ok(quote! {
        #params_struct
        #const_tokens
        #row_tokens
        #fn_tokens
    })
}

/// `:execrows` → `async fn foo<E: AsExecutor>(mut db: E, [params]) -> Result<u64, sqlx::Error>`
pub fn gen_execrows(query: &QueryView<'_>, ctx: &Ctx<'_>) -> Result<TokenStream, Error> {
    let params = resolve_params(query.params.iter(), ctx)?;
    let fn_name = format_ident!("{}", to_snake_case(query.name));
    let (const_tokens, const_name) = sql_const(query.name, query.text);
    let (params_struct, arg_ident, fn_params) =
        build_fn_params(query.name, &params, ctx.row_derives())?;
    let binds = bind_calls(&params, arg_ident.as_ref());
    let dynamic_slice = has_dynamic_slice(ctx, query.text, &params);
    let fn_tokens = if dynamic_slice {
        let DynamicQuery {
            setup: sql_setup,
            binds: bind_setup,
        } = dynamic_query(ctx, query.text, &const_name, &params, arg_ident.as_ref())?;
        quote! {
            pub async fn #fn_name<E: AsExecutor>(mut db: E, #fn_params) -> Result<u64, sqlx::Error> {
                #sql_setup
                let mut query = sqlx::query(sqlx::AssertSqlSafe(sql));
                #bind_setup
                let result = query.execute(db.as_executor()).await?;
                Ok(result.rows_affected())
            }
        }
    } else {
        quote! {
            pub async fn #fn_name<E: AsExecutor>(mut db: E, #fn_params) -> Result<u64, sqlx::Error> {
                let result = sqlx::query(#const_name)
                    #binds
                    .execute(db.as_executor())
                    .await?;
                Ok(result.rows_affected())
            }
        }
    };
    Ok(quote! { #params_struct #const_tokens #fn_tokens })
}

/// `:execresult` → `async fn foo<E: AsExecutor>(mut db: E, [params]) -> Result<sqlx::postgres::PgQueryResult, sqlx::Error>`
pub fn gen_execresult(query: &QueryView<'_>, ctx: &Ctx<'_>) -> Result<TokenStream, Error> {
    let params = resolve_params(query.params.iter(), ctx)?;
    let fn_name = format_ident!("{}", to_snake_case(query.name));
    let (const_tokens, const_name) = sql_const(query.name, query.text);
    let (params_struct, arg_ident, fn_params) =
        build_fn_params(query.name, &params, ctx.row_derives())?;
    let binds = bind_calls(&params, arg_ident.as_ref());
    let dynamic_slice = has_dynamic_slice(ctx, query.text, &params);
    let result_ty = ctx.engine.query_result_type();
    let fn_tokens = if dynamic_slice {
        let DynamicQuery {
            setup: sql_setup,
            binds: bind_setup,
        } = dynamic_query(ctx, query.text, &const_name, &params, arg_ident.as_ref())?;
        quote! {
            pub async fn #fn_name<E: AsExecutor>(mut db: E, #fn_params) -> Result<#result_ty, sqlx::Error> {
                #sql_setup
                let mut query = sqlx::query(sqlx::AssertSqlSafe(sql));
                #bind_setup
                query.execute(db.as_executor()).await
            }
        }
    } else {
        quote! {
            pub async fn #fn_name<E: AsExecutor>(mut db: E, #fn_params) -> Result<#result_ty, sqlx::Error> {
                sqlx::query(#const_name)
                    #binds
                    .execute(db.as_executor())
                    .await
            }
        }
    };
    Ok(quote! { #params_struct #const_tokens #fn_tokens })
}

/// `:exec` → `async fn foo<E: AsExecutor>(mut db: E, [params]) -> Result<(), sqlx::Error>`
pub fn gen_exec(query: &QueryView<'_>, ctx: &Ctx<'_>) -> Result<TokenStream, Error> {
    let params = resolve_params(query.params.iter(), ctx)?;
    let fn_name = format_ident!("{}", to_snake_case(query.name));
    let sql = query.text;
    let (const_tokens, const_name) = sql_const(query.name, sql);

    let (params_struct, arg_ident, fn_params) =
        build_fn_params(query.name, &params, ctx.row_derives())?;

    let binds = bind_calls(&params, arg_ident.as_ref());
    let dynamic_slice = has_dynamic_slice(ctx, query.text, &params);

    let fn_tokens = if dynamic_slice {
        let DynamicQuery {
            setup: sql_setup,
            binds: bind_setup,
        } = dynamic_query(ctx, query.text, &const_name, &params, arg_ident.as_ref())?;
        quote! {
            pub async fn #fn_name<E: AsExecutor>(mut db: E, #fn_params) -> Result<(), sqlx::Error> {
                #sql_setup
                let mut query = sqlx::query(sqlx::AssertSqlSafe(sql));
                #bind_setup
                query.execute(db.as_executor()).await?;
                Ok(())
            }
        }
    } else {
        quote! {
            pub async fn #fn_name<E: AsExecutor>(mut db: E, #fn_params) -> Result<(), sqlx::Error> {
                sqlx::query(#const_name)
                    #binds
                    .execute(db.as_executor())
                    .await?;
                Ok(())
            }
        }
    };

    Ok(quote! {
        #params_struct
        #const_tokens
        #fn_tokens
    })
}

/// `:execlastid` → `async fn foo<E: AsExecutor>(mut db: E, [params]) -> Result<T, sqlx::Error>`
///
/// How `T` is produced depends on the engine. PostgreSQL has no last-insert-id
/// concept, so sqlc requires a `RETURNING` clause and the value comes back as a
/// result column. MySQL reports it on the query result instead, and the query
/// has no result columns at all.
pub fn gen_execlastid(query: &QueryView<'_>, ctx: &Ctx<'_>) -> Result<TokenStream, Error> {
    let params = resolve_params(query.params.iter(), ctx)?;
    let fn_name = format_ident!("{}", to_snake_case(query.name));
    let (const_tokens, const_name) = sql_const(query.name, query.text);
    let (params_struct, arg_ident, fn_params) =
        build_fn_params(query.name, &params, ctx.row_derives())?;
    let binds = bind_calls(&params, arg_ident.as_ref());
    let dynamic_slice = has_dynamic_slice(ctx, query.text, &params);
    let dynamic = dynamic_slice
        .then(|| dynamic_query(ctx, query.text, &const_name, &params, arg_ident.as_ref()))
        .transpose()?;
    let sql_setup = dynamic.as_ref().map(|d| &d.setup);
    let bind_setup = dynamic.as_ref().map(|d| &d.binds);

    let (ret_ty, body) = match ctx.engine.last_insert_id() {
        LastInsertId::ReturningColumn => {
            let cols = resolve_columns(query.columns.iter(), ctx)?;
            let (_, first_resolved) = cols.flat.first().ok_or_else(|| {
                Error::Codegen(format!(
                    ":execlastid query '{}' has no result columns; \
                     PostgreSQL needs a RETURNING clause",
                    query.name
                ))
            })?;
            let ret_ty: syn::Type = parse_str(&first_resolved.rust_type).map_err(|e| {
                Error::Codegen(format!(
                    "invalid return type '{}': {e}",
                    first_resolved.rust_type
                ))
            })?;
            let body = if dynamic_slice {
                quote! {
                    let mut query = sqlx::query_as(sqlx::AssertSqlSafe(sql));
                    #bind_setup
                    let (_row,): (#ret_ty,) = query.fetch_one(db.as_executor()).await?;
                    Ok(_row)
                }
            } else {
                quote! {
                    let (_row,): (#ret_ty,) = sqlx::query_as(#const_name)
                        #binds
                        .fetch_one(db.as_executor())
                        .await?;
                    Ok(_row)
                }
            };
            (quote! { #ret_ty }, body)
        }
        LastInsertId::MySqlLastInsertId => {
            let body = if dynamic_slice {
                quote! {
                    let mut query = sqlx::query(sqlx::AssertSqlSafe(sql));
                    #bind_setup
                    let result = query.execute(db.as_executor()).await?;
                    Ok(result.last_insert_id())
                }
            } else {
                quote! {
                    let result = sqlx::query(#const_name)
                        #binds
                        .execute(db.as_executor())
                        .await?;
                    Ok(result.last_insert_id())
                }
            };
            (quote! { u64 }, body)
        }
    };

    let fn_tokens = quote! {
        pub async fn #fn_name<E: AsExecutor>(mut db: E, #fn_params) -> Result<#ret_ty, sqlx::Error> {
            #sql_setup
            #body
        }
    };
    Ok(quote! { #params_struct #const_tokens #fn_tokens })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::ResolvedType;

    fn slot_names(sql: &str) -> Vec<String> {
        scan_ordinal_slots(sql)
            .into_iter()
            .map(|slot| match slot {
                Slot::Scalar => "?".to_string(),
                Slot::Slice(name) => name,
            })
            .collect()
    }

    fn param(number: i32, name: &str, slice_name: Option<&str>) -> Param {
        Param {
            number,
            ident: field_ident(name),
            source_name: name.to_string(),
            is_slice: slice_name.is_some(),
            slice_name: slice_name.map(str::to_string),
            resolved: ResolvedType {
                rust_type: "i64".to_string(),
                borrowed_rust_type: None,
                copy_cheap: true,
            },
        }
    }

    #[test]
    fn scans_scalar_and_slice_positions_in_order() {
        let sql = "SELECT * FROM t WHERE a = ? AND b IN (/*SLICE:ids*/?) AND c = ?";
        assert_eq!(slot_names(sql), ["?", "ids", "?"]);
    }

    #[test]
    fn scans_multiple_slices() {
        let sql = "SELECT * FROM t WHERE a IN (/*SLICE:xs*/?) OR b IN (/*SLICE:ys*/?)";
        assert_eq!(slot_names(sql), ["xs", "ys"]);
    }

    #[test]
    fn ignores_question_marks_inside_string_literals() {
        let sql = "SELECT '?' AS q, \"a?b\" FROM t WHERE c = ?";
        assert_eq!(slot_names(sql), ["?"]);
    }

    #[test]
    fn ignores_escaped_quotes_inside_string_literals() {
        let sql = r"SELECT 'it\'s ?' FROM t WHERE c = ?";
        assert_eq!(slot_names(sql), ["?"]);
    }

    #[test]
    fn bind_order_follows_the_query_text_not_the_parameter_number() {
        // sqlc lifts sqlc.slice() out of the ordinal numbering, so the slice
        // gets the higher number even though it comes first in the text.
        let sql = "SELECT * FROM t WHERE id IN (/*SLICE:ids*/?) AND country = ?";
        let params = vec![param(2, "id", Some("ids")), param(1, "country", None)];
        let order = ordinal_bind_order(sql, &params).expect("bind order");
        assert_eq!(
            order
                .iter()
                .map(|p| p.source_name.as_str())
                .collect::<Vec<_>>(),
            ["id", "country"]
        );
    }

    #[test]
    fn bind_order_matches_slices_by_marker_name_not_column_name() {
        // `WHERE id IN (sqlc.slice(ids))` reports name="ids", original="id".
        let sql = "SELECT * FROM t WHERE id IN (/*SLICE:ids*/?)";
        let params = vec![param(1, "id", Some("ids"))];
        let order = ordinal_bind_order(sql, &params).expect("bind order");
        assert_eq!(order.len(), 1);
        assert_eq!(order[0].slice_name.as_deref(), Some("ids"));
    }

    #[test]
    fn bind_order_rejects_an_unmatched_slice_marker() {
        let sql = "SELECT * FROM t WHERE id IN (/*SLICE:ids*/?)";
        let Err(err) = ordinal_bind_order(sql, &[]) else {
            panic!("marker has no parameter");
        };
        assert!(
            err.to_string().contains("ids"),
            "expected the marker name in: {err}"
        );
    }

    #[test]
    fn bind_order_rejects_a_parameter_with_no_placeholder() {
        let sql = "SELECT * FROM t";
        let params = vec![param(1, "id", None)];
        let Err(err) = ordinal_bind_order(sql, &params) else {
            panic!("parameter has no placeholder");
        };
        assert!(
            err.to_string().contains("more parameters"),
            "expected a count mismatch in: {err}"
        );
    }
}
