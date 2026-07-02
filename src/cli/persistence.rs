use std::path::PathBuf;
use std::time::{Duration, SystemTime};

use cargo_reclaim::{
    InventoryOptions, Plan, PlanCommandKind, PlanInvocation, PolicyKind, SavePlanOptions,
    ScannerOptions, persist_plan, save_plan_to_path,
};

use super::{CliError, PlanMode};

const DEFAULT_EXPIRY: Duration = Duration::from_secs(24 * 60 * 60);

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct SavePlanRequest {
    pub path: PathBuf,
    pub expires_in: Duration,
}

impl SavePlanRequest {
    pub(super) fn new(path: PathBuf) -> Self {
        Self {
            path,
            expires_in: DEFAULT_EXPIRY,
        }
    }

    pub(super) fn set_expires_in(&mut self, expires_in: Duration) {
        self.expires_in = expires_in;
    }
}

pub(super) fn parse_duration(value: &str) -> Result<Duration, CliError> {
    let Some((number, suffix)) = value.split_at_checked(value.len().saturating_sub(1)) else {
        return Err(CliError::Usage("duration must not be empty".to_string()));
    };
    let amount = number
        .parse::<u64>()
        .map_err(|_| CliError::Usage(format!("invalid duration `{value}`")))?;
    if amount == 0 {
        return Err(CliError::Usage(
            "duration must be greater than zero".to_string(),
        ));
    }

    let seconds = match suffix {
        "s" => amount,
        "m" => amount.saturating_mul(60),
        "h" => amount.saturating_mul(60 * 60),
        "d" => amount.saturating_mul(24 * 60 * 60),
        _ => {
            return Err(CliError::Usage(format!(
                "invalid duration `{value}`; use s, m, h, or d"
            )));
        }
    };

    Ok(Duration::from_secs(seconds))
}

pub(super) fn save_plan(
    plan: &Plan,
    mode: PlanMode,
    policy: PolicyKind,
    scanner_options: &ScannerOptions,
    inventory_options: &InventoryOptions,
    request: &SavePlanRequest,
) -> Result<(), CliError> {
    let created_at = SystemTime::now();
    let expires_at = created_at
        .checked_add(request.expires_in)
        .ok_or_else(|| CliError::Usage("plan expiry duration is too large".to_string()))?;
    let command = match mode {
        PlanMode::Plan => PlanCommandKind::Plan,
        PlanMode::Scan => {
            return Err(CliError::Usage(
                "`--save-plan` is only supported by `plan`".to_string(),
            ));
        }
    };
    let invocation = PlanInvocation::new(command, policy, scanner_options, inventory_options);
    let document = persist_plan(
        plan,
        SavePlanOptions {
            created_at,
            expires_at,
            interactive_selection_modified: false,
            invocation,
        },
    )?;
    save_plan_to_path(&request.path, &document)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::parse_duration;

    #[test]
    fn parses_duration_units() -> Result<(), Box<dyn std::error::Error>> {
        assert_eq!(parse_duration("30s")?.as_secs(), 30);
        assert_eq!(parse_duration("30m")?.as_secs(), 30 * 60);
        assert_eq!(parse_duration("24h")?.as_secs(), 24 * 60 * 60);
        assert_eq!(parse_duration("7d")?.as_secs(), 7 * 24 * 60 * 60);
        Ok(())
    }

    #[test]
    fn rejects_invalid_duration() {
        for value in ["", "0s", "10", "1w", "abc"] {
            assert!(parse_duration(value).is_err());
        }
    }
}
