mod json;
mod labels;
mod terminal;

use std::io::Write;

use cargo_reclaim::{Plan, PolicyKind};

use super::{CliError, OutputFormat, PlanMode};

pub(super) fn write_help(output: &mut impl Write) -> Result<(), CliError> {
    terminal::write_help(output)
}

pub(super) fn write_plan(
    output: &mut impl Write,
    plan: &Plan,
    policy: PolicyKind,
    mode: PlanMode,
    format: OutputFormat,
) -> Result<(), CliError> {
    match format {
        OutputFormat::Terminal => terminal::write_plan(output, plan, policy, mode),
        OutputFormat::Json => json::write_plan(output, plan, policy, mode),
    }
}
