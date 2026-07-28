use std::process::Command;

#[test]
fn rust_binary_fails_closed_when_login_mode_is_requested() -> Result<(), Box<dyn std::error::Error>>
{
    // Java's relaxed binding treats every one of these spellings as `true`, so
    // each must refuse to start the unauthenticated Rust service.
    for requested in ["true", "1", "on", "YES"] {
        let working_directory = tempfile::tempdir()?;
        let output = Command::new(env!("CARGO_BIN_EXE_rustling-processing"))
            .current_dir(working_directory.path())
            .env("RUSTLING_PORT", "0")
            .env("SECURITY_ENABLELOGIN", requested)
            .env_remove("SECURITY_ENABLE_LOGIN")
            .env_remove("DOCKER_ENABLE_SECURITY")
            .output()?;

        assert!(
            !output.status.success(),
            "SECURITY_ENABLELOGIN={requested} must refuse to start"
        );
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stderr.contains("secured login mode is not supported"),
            "unexpected startup failure for SECURITY_ENABLELOGIN={requested}: {stderr}"
        );
    }
    // A malformed value also refuses startup — Java's relaxed binding fails
    // the boot on the same input, and guessing "off" here would fail open.
    for malformed in ["banana", "2"] {
        let working_directory = tempfile::tempdir()?;
        let output = Command::new(env!("CARGO_BIN_EXE_rustling-processing"))
            .current_dir(working_directory.path())
            .env("RUSTLING_PORT", "0")
            .env("SECURITY_ENABLELOGIN", malformed)
            .env_remove("SECURITY_ENABLE_LOGIN")
            .env_remove("DOCKER_ENABLE_SECURITY")
            .output()?;

        assert!(
            !output.status.success(),
            "SECURITY_ENABLELOGIN={malformed} must refuse to start rather than fail open"
        );
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stderr.contains("must be a boolean"),
            "unexpected startup failure for SECURITY_ENABLELOGIN={malformed}: {stderr}"
        );
    }
    Ok(())
}
