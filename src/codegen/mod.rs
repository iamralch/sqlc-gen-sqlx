use std::collections::HashMap;

use crate::{
    catalog,
    config::Config,
    emit::FileEmitter,
    engine::Engine,
    error::Error,
    plugin::GenerateRequestView,
    types::{ColumnOverride, TypeMap},
};

mod batch;
mod composites;
mod copyfrom;
mod enums;
mod executor;
pub(crate) mod lifetimes;
mod query;

/// Everything the per-command generators need that is not the query itself.
pub(crate) struct Ctx<'a> {
    pub(crate) engine: Engine,
    pub(crate) config: &'a Config,
    pub(crate) type_map: &'a TypeMap,
    pub(crate) col_overrides: &'a HashMap<String, ColumnOverride>,
}

impl Ctx<'_> {
    /// Extra derives applied to row and params structs.
    pub(crate) fn row_derives(&self) -> &[String] {
        &self.config.row_derives
    }
}

pub fn generate(request: &GenerateRequestView<'_>, config: &Config) -> Result<String, Error> {
    let engine = match request.settings.as_option() {
        Some(settings) => Engine::from_name(settings.engine)?,
        None => Engine::default(),
    };

    let mut type_map = TypeMap::new(engine, &config.overrides, &config.copy_cheap_types);
    let catalog_info = catalog::walk(request, engine, &mut type_map)?;
    let col_overrides = crate::types::build_column_overrides(&config.overrides);
    let ctx = Ctx {
        engine,
        config,
        type_map: &type_map,
        col_overrides: &col_overrides,
    };
    let mut emitter = FileEmitter::new(request.sqlc_version, env!("CARGO_PKG_VERSION"));

    // Emit the AsExecutor trait + impls up front so query functions can reference it.
    emitter.push(executor::gen_as_executor(engine));

    // Emit type definitions before query code.
    for info in &catalog_info.enums {
        emitter.push(enums::gen_enum(info, engine, &config.enum_derives)?);
    }
    for info in &catalog_info.composites {
        emitter.push(composites::gen_composite(info, &config.composite_derives)?);
    }

    for q in request.queries.iter() {
        let tokens = match q.cmd {
            ":exec" => query::gen_exec(q, &ctx)?,
            ":execrows" => query::gen_execrows(q, &ctx)?,
            ":execresult" => query::gen_execresult(q, &ctx)?,
            ":execlastid" => query::gen_execlastid(q, &ctx)?,
            ":batchexec" => batch::gen_batchexec(q, &ctx)?,
            ":batchone" => batch::gen_batchone(q, &ctx)?,
            ":batchmany" => batch::gen_batchmany(q, &ctx)?,
            ":copyfrom" => copyfrom::gen_copyfrom(q, &ctx)?,
            ":one" => query::gen_one(q, &ctx)?,
            ":many" => query::gen_many(q, &ctx)?,
            cmd => {
                eprintln!("sqlc-gen-sqlx: skipping unsupported annotation {cmd}");
                continue;
            }
        };
        emitter.push(tokens);
    }

    emitter.finish()
}
