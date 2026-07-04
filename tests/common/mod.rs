use std::path::Path;
use std::process::Command;

pub fn cargo_reclaim_command(isolation_root: &Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_cargo-reclaim"));
    command.env("CARGO_HOME", isolation_root.join("cargo-home"));
    command.env_remove("CARGO_BUILD_TARGET_DIR");
    command.env_remove("CARGO_TARGET_DIR");
    command.env_remove("CARGO_BUILD_BUILD_DIR");
    command
}
