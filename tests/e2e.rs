#[path = "e2e/support/mod.rs"]
mod support;

use support::Engine;

#[tokio::test]
async fn generated_code_compiles_and_runs_against_each_engine()
-> Result<(), Box<dyn std::error::Error>> {
    let cases = support::load_cases()?;

    for engine in Engine::ALL {
        let engine_cases = cases
            .iter()
            .filter(|case| case.engine == engine)
            .collect::<Vec<_>>();
        if engine_cases.is_empty() {
            continue;
        }

        // One container per engine, shared by every case that targets it.
        let database = support::start_database(engine).await?;
        let database_url = database.url().await?;

        for case in engine_cases {
            let tempdir = tempfile::TempDir::new()?;
            let crate_root = support::write_generated_crate(&tempdir, case)?;
            if let Err(err) = support::run_generated_crate(&crate_root, case, &database_url) {
                return Err(format!("e2e case '{}' failed: {}", case.name, err).into());
            }
        }
    }

    Ok(())
}
