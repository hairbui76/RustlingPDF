//! Wiring tests for the executable's background maintenance startup path:
//! which periodic loops actually spawn for the open, stateless runtime.

use rustling_processing::{ProcessingRuntime, TimestampSettings, runtime_config::RuntimeConfig};
use tempfile::tempdir;

#[tokio::test]
async fn open_runtime_spawns_exactly_the_ephemeral_sweeps() -> Result<(), Box<dyn std::error::Error>>
{
    let directory = tempdir()?;
    let runtime_config = RuntimeConfig::from_files(
        directory.path().join("settings.yml"),
        directory.path().join("custom.yml"),
    );
    let runtime = ProcessingRuntime::with_runtime_config(
        1024 * 1024,
        TimestampSettings::default(),
        runtime_config,
    );

    // Job-result cleanup and the mobile-scanner session sweep always run, and
    // nothing else: the stateless server has no audit, storage, or policy
    // state to maintain.
    assert_eq!(runtime.spawn_background_maintenance(), 2);
    Ok(())
}
