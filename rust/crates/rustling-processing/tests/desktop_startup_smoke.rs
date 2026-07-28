use rustling_processing::runtime_metrics::application_version;
use std::{
    error::Error,
    fs,
    io::{BufRead as _, BufReader, Read as _, Write as _},
    net::{IpAddr, Ipv4Addr, SocketAddr, TcpStream},
    process::{Child, Command, ExitStatus, Stdio},
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};
use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System};

const HANDSHAKE_PREFIX: &str = "RustlingPDF running on port: ";
const STARTUP_TIMEOUT: Duration = Duration::from_secs(60);
const PARENT_EXIT_TIMEOUT: Duration = Duration::from_secs(10);
const PARENT_HELPER_BACKEND_VARIABLE: &str = "RUSTLING_TEST_PARENT_HELPER_BACKEND";
const PARENT_HELPER_DIRECTORY_VARIABLE: &str = "RUSTLING_TEST_PARENT_HELPER_DIRECTORY";
const PARENT_HELPER_INFO_VARIABLE: &str = "RUSTLING_TEST_PARENT_HELPER_INFO";
const SETTINGS_TEMPLATE: &str = include_str!("../resources/settings.yml.template");

struct ChildGuard(Child);

impl ChildGuard {
    fn child_mut(&mut self) -> &mut Child {
        &mut self.0
    }
}

impl Drop for ChildGuard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[test]
fn reports_desktop_port_without_rust_log_and_serves_app_config() -> Result<(), Box<dyn Error>> {
    let working_directory = tempfile::tempdir()?;
    let child = Command::new(env!("CARGO_BIN_EXE_rustling-processing"))
        .current_dir(working_directory.path())
        .env("RUSTLING_PORT", "0")
        .env("RUSTLING_BASE_PATH", working_directory.path())
        .env_remove("SERVER_PORT")
        .env_remove("RUST_LOG")
        .env_remove("TAURI_PARENT_PID")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let mut child = ChildGuard(child);
    let stdout = child
        .child_mut()
        .stdout
        .take()
        .ok_or("processing child did not expose stdout")?;
    let (line_sender, line_receiver) = mpsc::channel();
    let reader = thread::spawn(move || {
        for line in BufReader::new(stdout).lines() {
            if line_sender.send(line).is_err() {
                return;
            }
        }
    });

    let deadline = Instant::now() + STARTUP_TIMEOUT;
    let port = loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err("processing child did not report its desktop port before timeout".into());
        }
        let line = line_receiver.recv_timeout(remaining)??;
        if let Some(port) = line
            .split_once(HANDSHAKE_PREFIX)
            .and_then(|(_, value)| value.trim().parse::<u16>().ok())
        {
            break port;
        }
    };

    let address = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port);
    let response = request_app_config(address)?;
    assert!(response.starts_with("HTTP/1.1 200"), "{response}");
    assert!(
        response.contains("\"dependenciesReady\":true"),
        "{response}"
    );

    drop(line_receiver);
    drop(child);
    reader
        .join()
        .map_err(|_| "processing stdout reader thread panicked")?;
    Ok(())
}

/// Targeted coverage for the legacy `STIRLING_*` alias path: a deployment
/// still configured through the pre-rename spellings must boot, honour the
/// aliased values, and log exactly one deprecation line on stderr.
#[test]
fn legacy_stirling_environment_still_boots_and_warns_once() -> Result<(), Box<dyn Error>> {
    let working_directory = tempfile::tempdir()?;
    let mut child = ChildGuard(
        Command::new(env!("CARGO_BIN_EXE_rustling-processing"))
            .current_dir(working_directory.path())
            .env_remove("RUSTLING_PORT")
            .env_remove("RUSTLING_BASE_PATH")
            .env("STIRLING_PORT", "0")
            .env("STIRLING_BASE_PATH", working_directory.path())
            .env_remove("SERVER_PORT")
            .env_remove("RUST_LOG")
            .env_remove("TAURI_PARENT_PID")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?,
    );
    let stdout = child
        .child_mut()
        .stdout
        .take()
        .ok_or("processing child did not expose stdout")?;
    let stderr = child
        .child_mut()
        .stderr
        .take()
        .ok_or("processing child did not expose stderr")?;
    let stderr_reader = thread::spawn(move || {
        let mut collected = String::new();
        let _ = BufReader::new(stderr).read_to_string(&mut collected);
        collected
    });

    let port = BufReader::new(stdout)
        .lines()
        .map_while(Result::ok)
        .find_map(|line| {
            line.split_once(HANDSHAKE_PREFIX)
                .and_then(|(_, value)| value.trim().parse::<u16>().ok())
        })
        .ok_or("processing child ended without reporting its desktop port")?;
    // STIRLING_PORT=0 must reach the bind: an ignored alias would fall back
    // to the fixed default port instead of an ephemeral one.
    assert_ne!(port, 8_081, "legacy STIRLING_PORT=0 was not honoured");

    drop(child);
    let stderr_output = stderr_reader
        .join()
        .map_err(|_| "processing stderr reader thread panicked")?;
    let deprecation_lines: Vec<&str> = stderr_output
        .lines()
        .filter(|line| line.starts_with("deprecated: STIRLING"))
        .collect();
    assert_eq!(
        deprecation_lines.len(),
        1,
        "expected exactly one deprecation line, stderr was: {stderr_output}"
    );
    assert!(
        deprecation_lines[0].contains("STIRLING_PORT")
            && deprecation_lines[0].contains("STIRLING_BASE_PATH"),
        "deprecation line must list the legacy variables: {}",
        deprecation_lines[0]
    );
    Ok(())
}

/// When both spellings are set the `RUSTLING_*` one must win: the legacy
/// variable holds an unparseable port, so booting successfully proves the
/// primary spelling took precedence.
#[test]
fn primary_rustling_spelling_wins_over_legacy_value() -> Result<(), Box<dyn Error>> {
    let working_directory = tempfile::tempdir()?;
    let mut child = ChildGuard(
        Command::new(env!("CARGO_BIN_EXE_rustling-processing"))
            .current_dir(working_directory.path())
            .env("RUSTLING_PORT", "0")
            .env("STIRLING_PORT", "not-a-port")
            .env("RUSTLING_BASE_PATH", working_directory.path())
            .env_remove("SERVER_PORT")
            .env_remove("RUST_LOG")
            .env_remove("TAURI_PARENT_PID")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?,
    );
    let stdout = child
        .child_mut()
        .stdout
        .take()
        .ok_or("processing child did not expose stdout")?;
    let reported_ready = BufReader::new(stdout)
        .lines()
        .map_while(Result::ok)
        .any(|line| line.contains(HANDSHAKE_PREFIX));
    assert!(
        reported_ready,
        "backend must boot from RUSTLING_PORT even when the STIRLING_PORT alias holds garbage"
    );
    Ok(())
}

#[test]
fn initializes_missing_desktop_settings_before_reporting_ready() -> Result<(), Box<dyn Error>> {
    let working_directory = tempfile::tempdir()?;
    boot_desktop_until_ready(working_directory.path())?;

    let config_directory = working_directory.path().join("configs");
    let settings = fs::read_to_string(config_directory.join("settings.yml"))?;
    assert_settings_is_template_with_generated_identity(&settings)?;
    assert_eq!(fs::read(config_directory.join("custom_settings.yml"))?, b"");

    // Second boot against the SAME base path: the install identity must
    // persist. Regression for the empty `custom_settings.yml` (always created
    // by desktop startup) parsing to YAML `null` and blanking the merged
    // settings snapshot, which re-rolled key/UUID on every boot — after a
    // reboot `settings.yml` must be byte-identical to the first boot's output.
    boot_desktop_until_ready(working_directory.path())?;
    assert_eq!(
        fs::read_to_string(config_directory.join("settings.yml"))?,
        settings,
        "settings.yml must be byte-stable across desktop reboots"
    );
    assert_eq!(fs::read(config_directory.join("custom_settings.yml"))?, b"");
    Ok(())
}

/// Boots the desktop-mode binary against `working_directory` as its base path,
/// waits until it reports ready (the port handshake line, printed only after
/// settings initialization), then shuts it down.
fn boot_desktop_until_ready(working_directory: &std::path::Path) -> Result<(), Box<dyn Error>> {
    let child = Command::new(env!("CARGO_BIN_EXE_rustling-processing"))
        .current_dir(working_directory)
        .env("RUSTLING_PORT", "0")
        .env("RUSTLING_BASE_PATH", working_directory)
        .env("RUSTLING_PDF_TAURI_MODE", "true")
        .env_remove("SERVER_PORT")
        .env_remove("RUST_LOG")
        .env_remove("TAURI_PARENT_PID")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let mut child = ChildGuard(child);
    let stdout = child
        .child_mut()
        .stdout
        .take()
        .ok_or("processing child did not expose stdout")?;
    let reported_ready = BufReader::new(stdout)
        .lines()
        .map_while(Result::ok)
        .any(|line| line.contains(HANDSHAKE_PREFIX));
    assert!(reported_ready, "processing child did not report ready");
    Ok(())
}

/// First-boot contract for `settings.yml`: byte-identical to the bundled
/// template — banner and every comment preserved — EXCEPT the three value
/// lines inside the template's existing `AutomaticallyGenerated` section,
/// where the startup identity write puts a generated UUID-shaped `key` and
/// `UUID` and the canonical application version (never the crate version).
fn assert_settings_is_template_with_generated_identity(
    settings: &str,
) -> Result<(), Box<dyn Error>> {
    let template_lines: Vec<&str> = SETTINGS_TEMPLATE.split_inclusive('\n').collect();
    let settings_lines: Vec<&str> = settings.split_inclusive('\n').collect();
    assert_eq!(
        settings_lines.len(),
        template_lines.len(),
        "settings.yml must keep the template's exact line count"
    );

    let mut generated_key = None;
    let mut generated_uuid = None;
    for (line_number, (settings_line, template_line)) in
        settings_lines.iter().zip(&template_lines).enumerate()
    {
        match template_line.trim_end() {
            "  key: example" => {
                generated_key = settings_line
                    .trim_end()
                    .strip_prefix("  key: ")
                    .map(ToOwned::to_owned);
            }
            "  UUID: example" => {
                generated_uuid = settings_line
                    .trim_end()
                    .strip_prefix("  UUID: ")
                    .map(ToOwned::to_owned);
            }
            "  appVersion: 0.35.0" => {
                assert_eq!(
                    settings_line.trim_end(),
                    format!("  appVersion: {}", application_version()),
                    "appVersion must be the canonical application version"
                );
            }
            _ => {
                assert_eq!(
                    settings_line,
                    template_line,
                    "line {} must be byte-identical to the template",
                    line_number + 1
                );
            }
        }
    }

    let generated_key =
        generated_key.ok_or("the template's AutomaticallyGenerated.key line is missing")?;
    let generated_uuid =
        generated_uuid.ok_or("the template's AutomaticallyGenerated.UUID line is missing")?;
    assert!(
        is_uuid_shaped(&generated_key),
        "generated key {generated_key:?} must be UUID-shaped"
    );
    assert!(
        is_uuid_shaped(&generated_uuid),
        "generated UUID {generated_uuid:?} must be UUID-shaped"
    );
    assert_ne!(
        generated_key, generated_uuid,
        "key and UUID must be generated independently"
    );
    Ok(())
}

/// Canonical UUID shape: 36 characters, hyphens at the four canonical
/// positions, hex digits everywhere else.
fn is_uuid_shaped(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() == 36
        && bytes.iter().enumerate().all(|(index, byte)| match index {
            8 | 13 | 18 | 23 => *byte == b'-',
            _ => byte.is_ascii_hexdigit(),
        })
}

#[test]
fn exits_after_its_short_lived_desktop_parent() -> Result<(), Box<dyn Error>> {
    let working_directory = tempfile::tempdir()?;
    let helper_info_path = working_directory.path().join("backend-process.txt");
    let current_test_executable = std::env::current_exe()?;
    let mut helper = Command::new(current_test_executable)
        .arg("--exact")
        .arg("short_lived_parent_helper")
        .arg("--ignored")
        .arg("--test-threads=1")
        .env(
            PARENT_HELPER_BACKEND_VARIABLE,
            env!("CARGO_BIN_EXE_rustling-processing"),
        )
        .env(PARENT_HELPER_DIRECTORY_VARIABLE, working_directory.path())
        .env(PARENT_HELPER_INFO_VARIABLE, &helper_info_path)
        .stdout(Stdio::null())
        .spawn()?;
    let helper_status = wait_for_child_exit(&mut helper, STARTUP_TIMEOUT)?;
    assert!(helper_status.success(), "helper failed: {helper_status}");

    let helper_info = fs::read_to_string(&helper_info_path)?;
    let mut fields = helper_info.split_ascii_whitespace();
    let backend_process_id = fields
        .next()
        .ok_or("helper did not report the backend process ID")?
        .parse::<u32>()?;
    let backend_port = fields
        .next()
        .ok_or("helper did not report the backend port")?
        .parse::<u16>()?;
    let backend_start_time = fields
        .next()
        .ok_or("helper did not report the backend process start time")?
        .parse::<u64>()?;
    assert!(fields.next().is_none(), "unexpected helper output");

    wait_for_process_exit(backend_process_id, backend_start_time, PARENT_EXIT_TIMEOUT)?;
    assert!(
        TcpStream::connect_timeout(
            &SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), backend_port),
            Duration::from_millis(250)
        )
        .is_err(),
        "backend still accepted connections after its desktop parent exited"
    );
    Ok(())
}

#[test]
#[ignore = "spawned as a short-lived parent by exits_after_its_short_lived_desktop_parent"]
fn short_lived_parent_helper() -> Result<(), Box<dyn Error>> {
    let backend = rustling_processing::env_compat::var_os(PARENT_HELPER_BACKEND_VARIABLE)
        .ok_or("parent helper backend path is missing")?;
    let working_directory =
        rustling_processing::env_compat::var_os(PARENT_HELPER_DIRECTORY_VARIABLE)
            .ok_or("parent helper working directory is missing")?;
    let helper_info_path = rustling_processing::env_compat::var_os(PARENT_HELPER_INFO_VARIABLE)
        .ok_or("parent helper info path is missing")?;
    let mut backend = Command::new(backend)
        .current_dir(&working_directory)
        .env("RUSTLING_PORT", "0")
        .env("RUSTLING_BASE_PATH", &working_directory)
        .env("TAURI_PARENT_PID", std::process::id().to_string())
        .env_remove("SERVER_PORT")
        .env_remove("RUST_LOG")
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()?;
    let backend_process_id = backend.id();
    let stdout = backend
        .stdout
        .take()
        .ok_or("processing child did not expose stdout")?;
    let backend_port = BufReader::new(stdout)
        .lines()
        .map_while(Result::ok)
        .find_map(|line| {
            line.split_once(HANDSHAKE_PREFIX)
                .and_then(|(_, value)| value.trim().parse::<u16>().ok())
        })
        .ok_or("processing child ended without reporting its desktop port")?;
    let backend_start_time = observed_process_start_time(backend_process_id)
        .ok_or("processing child was not visible while its parent was alive")?;
    fs::write(
        helper_info_path,
        format!("{backend_process_id} {backend_port} {backend_start_time}\n"),
    )?;

    // Dropping Child intentionally leaves the backend running. Returning ends
    // this helper process, which must be observed through TAURI_PARENT_PID.
    drop(backend);
    Ok(())
}

fn wait_for_child_exit(child: &mut Child, timeout: Duration) -> Result<ExitStatus, Box<dyn Error>> {
    let deadline = Instant::now() + timeout;
    loop {
        if let Some(status) = child.try_wait()? {
            return Ok(status);
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Err("short-lived parent helper did not exit before timeout".into());
        }
        thread::sleep(Duration::from_millis(25));
    }
}

fn wait_for_process_exit(
    process_id: u32,
    expected_start_time: u64,
    timeout: Duration,
) -> Result<(), Box<dyn Error>> {
    let deadline = Instant::now() + timeout;
    loop {
        let observed_start_time = observed_process_start_time(process_id);
        if observed_start_time != Some(expected_start_time) {
            return Ok(());
        }
        if Instant::now() >= deadline {
            terminate_process(process_id, expected_start_time);
            return Err(format!(
                "backend process {process_id} did not exit after its desktop parent"
            )
            .into());
        }
        thread::sleep(Duration::from_millis(50));
    }
}

fn observed_process_start_time(process_id: u32) -> Option<u64> {
    let pid = Pid::from_u32(process_id);
    let mut system = System::new();
    system.refresh_processes_specifics(
        ProcessesToUpdate::Some(&[pid]),
        true,
        ProcessRefreshKind::nothing(),
    );
    system.process(pid).map(sysinfo::Process::start_time)
}

fn terminate_process(process_id: u32, expected_start_time: u64) {
    let pid = Pid::from_u32(process_id);
    let mut system = System::new();
    system.refresh_processes_specifics(
        ProcessesToUpdate::Some(&[pid]),
        true,
        ProcessRefreshKind::nothing(),
    );
    if let Some(process) = system.process(pid)
        && process.start_time() == expected_start_time
    {
        let _ = process.kill();
    }
}

fn request_app_config(address: SocketAddr) -> Result<String, Box<dyn Error>> {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        match TcpStream::connect_timeout(&address, Duration::from_millis(250)) {
            Ok(mut stream) => {
                stream.set_read_timeout(Some(Duration::from_secs(5)))?;
                stream.write_all(
                    b"GET /api/v1/config/app-config HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n",
                )?;
                let mut response = String::new();
                stream.read_to_string(&mut response)?;
                return Ok(response);
            }
            Err(_) if Instant::now() < deadline => {
                thread::sleep(Duration::from_millis(50));
            }
            Err(error) => return Err(error.into()),
        }
    }
}
