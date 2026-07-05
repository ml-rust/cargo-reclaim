use std::error::Error;
use std::process::Command;

#[test]
fn help_leads_with_root_cleanup_and_list_without_targets_command() -> Result<(), Box<dyn Error>> {
    let output = Command::new(env!("CARGO_BIN_EXE_cargo-reclaim"))
        .arg("--help")
        .output()?;

    assert!(output.status.success());
    assert!(String::from_utf8(output.stderr)?.is_empty());
    let stdout = String::from_utf8(output.stdout)?;
    assert!(stdout.contains("cargo-reclaim [OPTIONS] [ROOT ...]"));
    assert!(stdout.contains("cargo-reclaim list [OPTIONS] [ROOT ...]"));
    assert!(stdout.contains("[ROOT ...]  Open the TTY cleanup assistant"));
    assert!(!stdout.contains("targets>"));
    assert!(!stdout.contains("  targets  "));
    Ok(())
}
