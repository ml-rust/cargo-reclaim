use std::io::Write;

use super::CliError;

pub(super) fn write_error_json(output: &mut impl Write, error: &CliError) -> Result<(), CliError> {
    let document = serde_json::json!({
        "command": "cargo-reclaim",
        "error": {
            "kind": error.kind_label(),
            "message": error.to_string(),
        },
        "exit_code": error.exit_code_value(),
    });
    serde_json::to_writer(&mut *output, &document)?;
    writeln!(output)?;
    Ok(())
}
