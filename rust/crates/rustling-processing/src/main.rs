use std::{
    env, io,
    net::{IpAddr, Ipv4Addr, SocketAddr},
};

use rustling_processing::{
    ProcessingRuntime, max_upload_bytes_from_environment, runtime_config::RuntimeConfig,
};
use tracing::info;
use tracing_subscriber::EnvFilter;

mod desktop_settings;
mod parent_process;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .init();

    rustling_processing::env_compat::warn_once_on_legacy_environment();
    desktop_settings::initialize_from_environment()?;
    let bootstrap_config = RuntimeConfig::from_environment();
    // Legacy login/MCP/server-state settings are ignored, never refused: the
    // bundled desktop template still ships `security.enableLogin: true` on
    // existing installs and a hard failure would brick them on upgrade.
    bootstrap_config.warn_on_ignored_legacy_settings();
    // Install identity (Java `InitialSetup`): a UUID and machine key in
    // AutomaticallyGenerated.* plus the running app version. Only the desktop
    // sidecar persists it into its own local settings.yml (fail-open there);
    // the server deployment is stateless and resolves it in memory per boot.
    if desktop_settings::tauri_mode_active() {
        match bootstrap_config.initialize_generated_identity() {
            Ok(identity) => info!(
                uuid = %identity.uuid,
                app_version = %identity.app_version,
                is_new_server = identity.is_new_server,
                "install identity ready"
            ),
            Err(error) => tracing::warn!(
                %error,
                "could not persist the generated install identity; continuing with an ephemeral one"
            ),
        }
    } else {
        let identity = bootstrap_config.ephemeral_generated_identity();
        info!(
            uuid = %identity.uuid,
            app_version = %identity.app_version,
            is_new_server = identity.is_new_server,
            "install identity ready (in-memory)"
        );
    }
    let parent_process = parent_process::ParentProcessWatcher::from_environment()?;

    let address = SocketAddr::new(configured_host()?, configured_port()?);
    let listener = tokio::net::TcpListener::bind(address).await?;
    let address = listener.local_addr()?;
    let runtime = ProcessingRuntime::from_environment_with_dependency_discovery(
        max_upload_bytes_from_environment(),
    );
    // Background maintenance loops ported from the Java @Scheduled tasks plus
    // the one-shot startup sweep of crash-abandoned temp artifacts.
    let maintenance_loops = runtime.spawn_background_maintenance();
    info!(maintenance_loops, "spawned background maintenance");
    info!(%address, "starting RustlingPDF processing service");
    // Desktop discovers an ephemeral sidecar port from this stable handshake.
    // It must not depend on RUST_LOG: EnvFilter defaults to ERROR when that
    // variable is absent, which would otherwise leave the desktop waiting
    // forever for an INFO event that never reaches the child-process pipe.
    println!("RustlingPDF running on port: {}", address.port());
    let service = runtime
        .into_router()
        .into_make_service_with_connect_info::<SocketAddr>();
    let server = axum::serve(listener, service);
    if let Some(parent_process) = parent_process {
        tokio::select! {
            result = server => result?,
            () = parent_process.wait_until_exit() => {
                info!("Tauri parent process exited; shutting down RustlingPDF processing service");
            }
        }
    } else {
        server.await?;
    }
    Ok(())
}

fn configured_host() -> Result<IpAddr, io::Error> {
    for variable in ["RUSTLING_HOST", "SERVER_ADDRESS"] {
        match rustling_processing::env_compat::var(variable) {
            Ok(value) => return parse_host(variable, &value),
            Err(env::VarError::NotPresent) => {}
            Err(env::VarError::NotUnicode(_)) => {
                return Err(invalid_environment_unicode(variable));
            }
        }
    }
    Ok(IpAddr::V4(Ipv4Addr::LOCALHOST))
}

fn configured_port() -> Result<u16, io::Error> {
    for variable in ["RUSTLING_PORT", "SERVER_PORT"] {
        match rustling_processing::env_compat::var(variable) {
            Ok(value) => return parse_port(variable, &value),
            Err(env::VarError::NotPresent) => {}
            Err(env::VarError::NotUnicode(_)) => {
                return Err(invalid_environment_unicode(variable));
            }
        }
    }
    Ok(8_081)
}

fn parse_host(variable: &str, value: &str) -> Result<IpAddr, io::Error> {
    value.trim().parse::<IpAddr>().map_err(|error| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("{variable} must be a valid IP address: {error}"),
        )
    })
}

fn parse_port(variable: &str, value: &str) -> Result<u16, io::Error> {
    value.trim().parse::<u16>().map_err(|error| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("{variable} must be a valid TCP port: {error}"),
        )
    })
}

fn invalid_environment_unicode(variable: &str) -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidInput,
        format!("{variable} must contain valid Unicode"),
    )
}

#[cfg(test)]
mod tests {
    use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

    use super::{parse_host, parse_port};

    #[test]
    fn parses_loopback_and_container_hosts() {
        assert_eq!(
            parse_host("RUSTLING_HOST", "127.0.0.1").ok(),
            Some(IpAddr::V4(Ipv4Addr::LOCALHOST))
        );
        assert_eq!(
            parse_host("RUSTLING_HOST", " 0.0.0.0 ").ok(),
            Some(IpAddr::V4(Ipv4Addr::UNSPECIFIED))
        );
        assert_eq!(
            parse_host("SERVER_ADDRESS", "::").ok(),
            Some(IpAddr::V6(Ipv6Addr::UNSPECIFIED))
        );
        assert!(parse_host("RUSTLING_HOST", "localhost").is_err());
    }

    #[test]
    fn parses_fixed_and_ephemeral_ports() {
        assert_eq!(parse_port("RUSTLING_PORT", "8081").ok(), Some(8_081));
        assert_eq!(parse_port("RUSTLING_PORT", " 0 ").ok(), Some(0));
        assert!(parse_port("RUSTLING_PORT", "not-a-port").is_err());
    }
}
