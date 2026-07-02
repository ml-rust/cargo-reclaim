use std::path::PathBuf;

use super::inventory::inventory_cargo_home;
use super::model::{
    CARGO_HOME_REPORT_SCHEMA_VERSION, CargoHomeClass, CargoHomeError, CargoHomeInput,
    CargoHomeRecommendation, CargoHomeReport, CargoHomeTotals,
};
use super::resolve::{CargoHomeResolveRequest, resolve_cargo_home};

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CargoHomeReportRequest {
    pub cargo_home: Option<PathBuf>,
}

pub fn build_cargo_home_report(
    request: CargoHomeReportRequest,
) -> Result<CargoHomeReport, CargoHomeError> {
    let input = resolve_cargo_home(CargoHomeResolveRequest {
        explicit_path: request.cargo_home,
    })?;
    report_for_input(input)
}

fn report_for_input(input: CargoHomeInput) -> Result<CargoHomeReport, CargoHomeError> {
    let (entries, problems) = inventory_cargo_home(input.clone())?;
    let totals = totals_for_entries(&entries, problems.len());
    let recommendations = recommendations(&totals);
    Ok(CargoHomeReport {
        schema_version: CARGO_HOME_REPORT_SCHEMA_VERSION,
        input,
        entries,
        totals,
        recommendations,
        problems,
    })
}

fn totals_for_entries(
    entries: &[super::model::CargoHomeEntry],
    problem_count: usize,
) -> CargoHomeTotals {
    let mut total_bytes = 0u64;
    let mut cache_bytes = 0u64;
    let mut preserved_bytes = 0u64;
    let mut skipped_count = 0usize;
    let mut known_cache_entry_count = 0usize;
    for entry in entries {
        total_bytes = total_bytes.saturating_add(entry.size_bytes);
        if entry.preserved {
            preserved_bytes = preserved_bytes.saturating_add(entry.size_bytes);
        }
        if entry.skipped {
            skipped_count += 1;
        }
        if is_cache_class(entry.class) {
            cache_bytes = cache_bytes.saturating_add(entry.size_bytes);
            known_cache_entry_count += 1;
        }
    }
    CargoHomeTotals {
        entry_count: entries.len(),
        total_bytes,
        cache_bytes,
        preserved_bytes,
        skipped_count,
        problem_count,
        known_cache_entry_count,
    }
}

fn recommendations(totals: &CargoHomeTotals) -> Vec<CargoHomeRecommendation> {
    if totals.known_cache_entry_count == 0 {
        return Vec::new();
    }
    vec![CargoHomeRecommendation {
        message: "Review Cargo cache.auto-clean-frequency to let Cargo manage global cache cleanup; cargo-reclaim did not modify Cargo config or files.".to_string(),
    }]
}

fn is_cache_class(class: CargoHomeClass) -> bool {
    matches!(
        class,
        CargoHomeClass::RegistryIndex
            | CargoHomeClass::RegistryCache
            | CargoHomeClass::RegistrySource
            | CargoHomeClass::GitDatabase
            | CargoHomeClass::GitCheckouts
    )
}
