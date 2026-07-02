use std::io::Write;
use std::time::SystemTime;

use cargo_reclaim::{
    ActiveObservationProvider, build_plan_from_roots_with_active_observation_provider,
    resolve_command_toolchain_hash_options,
};

use super::output::write_plan;
use super::persistence::{SavePlanContext, save_plan};
use super::{CliError, PlanCommand};

pub(super) fn run_plan_command(
    command: PlanCommand,
    stdout: &mut impl Write,
    active_observation_provider: &impl ActiveObservationProvider,
) -> Result<(), CliError> {
    let PlanCommand {
        mode,
        roots,
        policy,
        output_format,
        save_plan: save_plan_request,
        config_path,
        config_version,
        scanner_options,
        inventory_options,
        mut planner_options,
    } = command;
    resolve_command_toolchain_hash_options(&mut planner_options)?;
    let plan = build_plan_from_roots_with_active_observation_provider(
        roots,
        policy,
        &scanner_options,
        &inventory_options,
        &planner_options,
        active_observation_provider,
        SystemTime::now(),
    )?;
    if let Some(request) = save_plan_request.as_ref() {
        save_plan(
            &plan,
            SavePlanContext {
                mode,
                policy,
                scanner_options: &scanner_options,
                inventory_options: &inventory_options,
                planner_options: &planner_options,
                config_path: config_path.as_deref(),
                config_version,
                request,
            },
        )?;
    }
    write_plan(stdout, &plan, policy, mode, output_format)?;
    Ok(())
}
