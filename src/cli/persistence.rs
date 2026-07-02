use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use cargo_reclaim::{
    InventoryOptions, Plan, PlanCommandKind, PlanInvocation, PlannerOptions, PolicyKind,
    SavePlanOptions, ScannerOptions, persist_plan, save_plan_to_path,
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

#[derive(Debug)]
pub(super) struct SavePlanContext<'a> {
    pub mode: PlanMode,
    pub policy: PolicyKind,
    pub scanner_options: &'a ScannerOptions,
    pub inventory_options: &'a InventoryOptions,
    pub planner_options: &'a PlannerOptions,
    pub config_path: Option<&'a Path>,
    pub config_version: Option<u16>,
    pub request: &'a SavePlanRequest,
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

pub(super) fn parse_days(value: &str) -> Result<Duration, CliError> {
    let days = value
        .parse::<u64>()
        .map_err(|_| CliError::Usage(format!("invalid day count `{value}`")))?;
    if days == 0 {
        return Err(CliError::Usage(
            "day count must be greater than zero".to_string(),
        ));
    }
    Ok(Duration::from_secs(days.saturating_mul(24 * 60 * 60)))
}

pub(super) fn parse_size(value: &str) -> Result<u64, CliError> {
    let trimmed = value.trim();
    let (amount, unit) = split_amount_and_unit(trimmed)
        .ok_or_else(|| CliError::Usage(format!("invalid size `{value}`")))?;
    let amount = amount
        .parse::<u64>()
        .map_err(|_| CliError::Usage(format!("invalid size `{value}`")))?;
    if amount == 0 {
        return Err(CliError::Usage(
            "size must be greater than zero".to_string(),
        ));
    }
    let multiplier = match unit {
        "B" | "byte" | "bytes" => 1_u64,
        "KiB" => 1024,
        "MiB" => 1024_u64.saturating_pow(2),
        "GiB" => 1024_u64.saturating_pow(3),
        "TiB" => 1024_u64.saturating_pow(4),
        _ => {
            return Err(CliError::Usage(format!(
                "invalid size `{value}`; use B, KiB, MiB, GiB, or TiB"
            )));
        }
    };
    amount
        .checked_mul(multiplier)
        .ok_or_else(|| CliError::Usage(format!("invalid size `{value}`")))
}

fn split_amount_and_unit(value: &str) -> Option<(&str, &str)> {
    if let Some((amount, unit)) = value.split_once(' ') {
        let unit = unit.trim();
        if amount.is_empty() || unit.is_empty() || unit.contains(' ') {
            return None;
        }
        return Some((amount, unit));
    }

    let split_at = value.find(|character: char| !character.is_ascii_digit())?;
    let (amount, unit) = value.split_at(split_at);
    if amount.is_empty() || unit.is_empty() {
        return None;
    }
    Some((amount, unit))
}

pub(super) fn save_plan(plan: &Plan, context: SavePlanContext<'_>) -> Result<(), CliError> {
    let created_at = SystemTime::now();
    let expires_at = created_at
        .checked_add(context.request.expires_in)
        .ok_or_else(|| CliError::Usage("plan expiry duration is too large".to_string()))?;
    let command = match context.mode {
        PlanMode::Plan => PlanCommandKind::Plan,
        PlanMode::Scan => {
            return Err(CliError::Usage(
                "`--save-plan` is only supported by `plan`".to_string(),
            ));
        }
    };
    let mut invocation = PlanInvocation::new(
        command,
        context.policy,
        context.scanner_options,
        context.inventory_options,
        context.planner_options,
    );
    if let (Some(config_path), Some(config_version)) = (context.config_path, context.config_version)
    {
        invocation = invocation.with_config(config_path, config_version);
    }
    let document = persist_plan(
        plan,
        SavePlanOptions {
            created_at,
            expires_at,
            interactive_selection_modified: false,
            invocation,
        },
    )?;
    save_plan_to_path(&context.request.path, &document)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{parse_days, parse_duration, parse_size};

    #[test]
    fn parses_duration_units() -> Result<(), Box<dyn std::error::Error>> {
        assert_eq!(parse_duration("30s")?.as_secs(), 30);
        assert_eq!(parse_duration("30m")?.as_secs(), 30 * 60);
        assert_eq!(parse_duration("24h")?.as_secs(), 24 * 60 * 60);
        assert_eq!(parse_duration("7d")?.as_secs(), 7 * 24 * 60 * 60);
        Ok(())
    }

    #[test]
    fn parses_day_counts() -> Result<(), Box<dyn std::error::Error>> {
        assert_eq!(parse_days("3")?.as_secs(), 3 * 24 * 60 * 60);
        Ok(())
    }

    #[test]
    fn parses_size_units() -> Result<(), Box<dyn std::error::Error>> {
        assert_eq!(parse_size("1B")?, 1);
        assert_eq!(parse_size("2 KiB")?, 2 * 1024);
        assert_eq!(parse_size("3MiB")?, 3 * 1024 * 1024);
        Ok(())
    }

    #[test]
    fn rejects_invalid_duration() {
        for value in ["", "0s", "10", "1w", "abc"] {
            assert!(parse_duration(value).is_err());
        }
    }

    #[test]
    fn rejects_invalid_days_and_sizes() {
        for value in ["", "0", "1d", "abc"] {
            assert!(parse_days(value).is_err());
        }
        for value in ["", "0B", "10", "1GB", "abc"] {
            assert!(parse_size(value).is_err());
        }
    }
}
