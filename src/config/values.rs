use std::path::{Path, PathBuf};
use std::time::Duration;

use super::error::ConfigError;

pub(super) fn parse_config_duration(value: &str) -> Result<Duration, ConfigError> {
    let trimmed = value.trim();
    let Some((amount, unit)) = trimmed.split_once(' ') else {
        return parse_compact_duration(trimmed);
    };
    let amount = parse_positive_amount(amount, value)?;
    let seconds = match unit {
        "second" | "seconds" => amount,
        "minute" | "minutes" => amount.saturating_mul(60),
        "hour" | "hours" => amount.saturating_mul(60 * 60),
        "day" | "days" => amount.saturating_mul(24 * 60 * 60),
        _ => return Err(ConfigError::InvalidDuration(value.to_string())),
    };
    Ok(Duration::from_secs(seconds))
}

fn parse_compact_duration(value: &str) -> Result<Duration, ConfigError> {
    let Some((number, suffix)) = value.split_at_checked(value.len().saturating_sub(1)) else {
        return Err(ConfigError::InvalidDuration(value.to_string()));
    };
    let amount = parse_positive_amount(number, value)?;
    let seconds = match suffix {
        "s" => amount,
        "m" => amount.saturating_mul(60),
        "h" => amount.saturating_mul(60 * 60),
        "d" => amount.saturating_mul(24 * 60 * 60),
        _ => return Err(ConfigError::InvalidDuration(value.to_string())),
    };
    Ok(Duration::from_secs(seconds))
}

fn parse_positive_amount(value: &str, original: &str) -> Result<u64, ConfigError> {
    let amount = value
        .parse::<u64>()
        .map_err(|_| ConfigError::InvalidDuration(original.to_string()))?;
    if amount == 0 {
        return Err(ConfigError::InvalidDuration(original.to_string()));
    }
    Ok(amount)
}

pub(super) fn parse_config_size(value: &str) -> Result<u64, ConfigError> {
    let trimmed = value.trim();
    let (amount, unit) = split_amount_and_unit(trimmed)
        .ok_or_else(|| ConfigError::InvalidSize(value.to_string()))?;
    let amount = amount
        .parse::<u64>()
        .map_err(|_| ConfigError::InvalidSize(value.to_string()))?;
    if amount == 0 {
        return Err(ConfigError::InvalidSize(value.to_string()));
    }
    let multiplier = match unit {
        "B" | "byte" | "bytes" => 1_u64,
        "KiB" => 1024,
        "MiB" => 1024_u64.saturating_pow(2),
        "GiB" => 1024_u64.saturating_pow(3),
        "TiB" => 1024_u64.saturating_pow(4),
        _ => return Err(ConfigError::InvalidSize(value.to_string())),
    };
    amount
        .checked_mul(multiplier)
        .ok_or_else(|| ConfigError::InvalidSize(value.to_string()))
}

pub(super) fn parse_config_percentage_basis_points(value: &str) -> Result<u16, ConfigError> {
    let trimmed = value.trim();
    let without_percent = trimmed
        .strip_suffix('%')
        .map(str::trim)
        .ok_or_else(|| ConfigError::InvalidPercentage(value.to_string()))?;
    let basis_points = parse_decimal_percentage_basis_points(without_percent)
        .ok_or_else(|| ConfigError::InvalidPercentage(value.to_string()))?;
    if basis_points == 0 || basis_points > 10_000 {
        return Err(ConfigError::InvalidPercentage(value.to_string()));
    }
    u16::try_from(basis_points).map_err(|_| ConfigError::InvalidPercentage(value.to_string()))
}

fn parse_decimal_percentage_basis_points(value: &str) -> Option<u32> {
    if value.is_empty() {
        return None;
    }
    let (whole, fractional) = value.split_once('.').unwrap_or((value, ""));
    if whole.is_empty()
        || !whole.chars().all(|character| character.is_ascii_digit())
        || !fractional
            .chars()
            .all(|character| character.is_ascii_digit())
        || fractional.len() > 2
    {
        return None;
    }
    let whole_basis_points = whole.parse::<u32>().ok()?.checked_mul(100)?;
    let fractional_basis_points = match fractional.len() {
        0 => 0,
        1 => fractional.parse::<u32>().ok()?.checked_mul(10)?,
        2 => fractional.parse::<u32>().ok()?,
        _ => return None,
    };
    whole_basis_points.checked_add(fractional_basis_points)
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

pub(super) fn expand_home(path: PathBuf) -> PathBuf {
    let text = path.to_string_lossy();
    let Some(rest) = text.strip_prefix("~/") else {
        return path;
    };
    let Some(home) = home_dir() else {
        return path;
    };
    home.join(rest)
}

pub(super) fn resolve_config_path(path: PathBuf, relative_base: Option<&Path>) -> PathBuf {
    let expanded = expand_home(path);
    if expanded.is_absolute() {
        return expanded;
    }
    relative_base
        .map(|base| base.join(&expanded))
        .unwrap_or(expanded)
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
}
