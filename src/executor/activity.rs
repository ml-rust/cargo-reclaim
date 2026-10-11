use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use crate::active_process::{
    ActiveObservationProvider, ActiveObservationScope, platform_active_observation_provider,
};
use crate::persistence::{PersistedPlan, PersistedPlanEntry};
use crate::planner::{ActiveObservation, TargetContext, active_skip_reason};
use crate::scanner::{ScanItem, ScannerOptions, scan_roots};

use super::report::{ApplyEntryResult, ApplyEntryStatus};
use super::revalidate::revalidate_entry;

/// Saved file snapshots cannot show that a different artifact in the same target
/// was written after planning. Refresh target activity before applying the plan.
pub(super) struct ApplyActivity {
    contexts: Vec<TargetContext>,
    observation: ActiveObservation,
    window: Option<Duration>,
    skipped: Vec<PathBuf>,
    now: SystemTime,
    error: Option<String>,
    checked: HashMap<PathBuf, Option<String>>,
}

impl ApplyActivity {
    pub(super) fn new(document: &PersistedPlan, now: SystemTime) -> Self {
        Self::with_provider(document, now, &platform_active_observation_provider())
    }

    fn with_provider(
        document: &PersistedPlan,
        now: SystemTime,
        provider: &impl ActiveObservationProvider,
    ) -> Self {
        let saved = &document.body.invocation.scanner_options;
        let mut options = ScannerOptions {
            follow_symlinks: saved.follow_symlinks,
            allow_name_only_targets: saved.allow_name_only_targets,
            cross_filesystems: saved.cross_filesystems,
            ignored_paths: saved.ignored_paths.iter().map(PathBuf::from).collect(),
            skipped_paths: saved.skipped_paths.iter().map(PathBuf::from).collect(),
        };
        let mut contexts = Vec::new();
        let mut error = None;
        for path in options
            .ignored_paths
            .iter_mut()
            .chain(&mut options.skipped_paths)
        {
            match std::path::absolute(&*path) {
                Ok(absolute) => *path = absolute,
                Err(cause) => error = Some(format!("cannot resolve exclusion path: {cause}")),
            }
        }
        let mut roots = Vec::new();
        for root in &document.body.plan.input.roots {
            match std::path::absolute(root) {
                Ok(path) => roots.push(path),
                Err(cause) => error = Some(format!("cannot resolve scan root: {cause}")),
            }
        }
        match scan_roots(roots, &options) {
            Ok(items) => {
                for item in items {
                    if let ScanItem::TargetCandidate(candidate) = item
                        && candidate.evidence.is_some()
                    {
                        let mut context = candidate
                            .target_context
                            .unwrap_or_else(|| TargetContext::new(&candidate.path));
                        if let Err(cause) = normalize_context(&mut context) {
                            error = Some(format!("cannot resolve target context: {cause}"));
                            continue;
                        }
                        contexts.push(context);
                    }
                }
            }
            Err(cause) => error = Some(format!("cannot recheck target activity: {cause}")),
        }
        let observation = provider.observe(&ActiveObservationScope::from_target_contexts(
            contexts.clone(),
        ));
        Self {
            contexts,
            observation,
            error,
            window: document
                .body
                .invocation
                .planner_options
                .recent_write_keep_window_seconds
                .map(Duration::from_secs),
            skipped: options.skipped_paths,
            now,
            checked: HashMap::new(),
        }
    }

    pub(super) fn revalidate(&mut self, entry: &PersistedPlanEntry) -> ApplyEntryResult {
        let result = revalidate_entry(entry);
        if result.status != ApplyEntryStatus::WouldDelete {
            return result;
        }
        if let Some(reason) = self.skip_reason(Path::new(&entry.snapshot.path)) {
            return ApplyEntryResult::new(
                entry.snapshot.path.clone(),
                entry.action.clone(),
                ApplyEntryStatus::SkipStalePlan,
                entry.snapshot.size_bytes,
                format!("skip_stale_plan: {reason}"),
            );
        }
        result
    }

    fn skip_reason(&mut self, path: &Path) -> Option<String> {
        if let Some(error) = &self.error {
            return Some(error.clone());
        }
        let path = match std::path::absolute(path) {
            Ok(path) => path,
            Err(error) => return Some(format!("cannot resolve planned path: {error}")),
        };
        let context = self
            .contexts
            .iter()
            .filter(|context| path.starts_with(&context.target_root))
            .max_by_key(|context| context.target_root.components().count());
        let Some(context) = context else {
            return self
                .window
                .map(|_| "cannot identify the target for recent-write revalidation".to_string());
        };
        let root = &context.target_root;
        if let Some(reason) = self.checked.get(root) {
            return reason.clone();
        }
        let reason = active_skip_reason(Some(context), &self.observation).or_else(|| {
            self.window.and_then(|window| {
                match has_recent_write(root, &self.skipped, self.now, window) {
                    Ok(true) => Some(
                        "target was written within the recent-write keep window after planning"
                            .to_string(),
                    ),
                    Ok(false) => None,
                    Err(error) => Some(format!("cannot recheck target writes: {error}")),
                }
            })
        });
        self.checked.insert(root.clone(), reason.clone());
        reason
    }
}

// Use the same absolute path base for process working directories, references,
// candidate paths and exclusions without following filesystem symlinks.
fn normalize_context(context: &mut TargetContext) -> std::io::Result<()> {
    context.target_root = std::path::absolute(&context.target_root)?;
    if let Some(path) = &mut context.project_root {
        *path = std::path::absolute(&*path)?;
    }
    if let Some(path) = &mut context.build_root {
        *path = std::path::absolute(&*path)?;
    }
    Ok(())
}

fn has_recent_write(
    root: &Path,
    skipped: &[PathBuf],
    now: SystemTime,
    window: Duration,
) -> std::io::Result<bool> {
    let mut pending = vec![root.to_path_buf()];
    while let Some(path) = pending.pop() {
        if skipped.iter().any(|skip| path.starts_with(skip)) {
            continue;
        }
        let metadata = fs::symlink_metadata(&path)?;
        if metadata.file_type().is_symlink() {
            continue;
        }
        let modified = metadata.modified()?;
        if now
            .duration_since(modified)
            .map_or(true, |age| age <= window)
        {
            return Ok(true);
        }
        if metadata.is_dir() {
            for entry in fs::read_dir(path)? {
                pending.push(entry?.path());
            }
        }
    }
    Ok(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::planner::{CargoTool, ObservedCargoProcess};

    fn guard(observation: ActiveObservation) -> ApplyActivity {
        let root = std::env::temp_dir().join("reclaim-activity-unit");
        ApplyActivity {
            contexts: vec![TargetContext::new(root.join("target")).with_project_root(&root)],
            observation,
            window: None,
            skipped: Vec::new(),
            now: SystemTime::now(),
            error: None,
            checked: HashMap::new(),
        }
    }

    #[test]
    fn relative_root_is_normalized_before_process_observation() {
        use crate::{
            InventoryOptions, Plan, PlanCommandKind, PlanInput, PlanInvocation, PlannerOptions,
            PolicyKind, SavePlanOptions, persist_plan,
        };
        struct Fixture(PathBuf);
        impl Drop for Fixture {
            fn drop(&mut self) {
                let _ = fs::remove_dir_all(&self.0);
            }
        }
        let unique = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let fixture = Fixture(PathBuf::from(format!("reclaim-relative-process-{unique}")));
        fs::create_dir_all(fixture.0.join("target")).unwrap();
        fs::write(
            fixture.0.join("Cargo.toml"),
            "[package]\nname = \"sample\"\n",
        )
        .unwrap();
        struct CargoInProject(PathBuf);
        impl ActiveObservationProvider for CargoInProject {
            fn observe(&self, scope: &ActiveObservationScope) -> ActiveObservation {
                let cwd = self.0.clone();
                assert!(
                    scope
                        .target_contexts()
                        .iter()
                        .any(|context| context.project_root.as_ref() == Some(&cwd))
                );
                for context in scope.target_contexts() {
                    assert!(context.target_root.is_absolute());
                    assert!(
                        context
                            .project_root
                            .as_ref()
                            .is_none_or(|p| p.is_absolute())
                    );
                    assert!(context.build_root.as_ref().is_none_or(|p| p.is_absolute()));
                }
                ActiveObservation::complete([
                    ObservedCargoProcess::new(CargoTool::Cargo).with_cwd(cwd)
                ])
            }
        }
        let now = SystemTime::now();
        let plan = Plan::new(PlanInput::from_root(&fixture.0).unwrap(), vec![]);
        let document = persist_plan(
            &plan,
            SavePlanOptions {
                created_at: now,
                expires_at: now + Duration::from_secs(3600),
                interactive_selection_modified: false,
                invocation: PlanInvocation::new(
                    PlanCommandKind::Plan,
                    PolicyKind::Balanced,
                    &ScannerOptions::default(),
                    &InventoryOptions::default(),
                    &PlannerOptions::default(),
                ),
            },
        )
        .unwrap();
        let provider = CargoInProject(std::path::absolute(&fixture.0).unwrap());
        let mut activity = ApplyActivity::with_provider(&document, now, &provider);
        assert!(
            activity
                .skip_reason(&fixture.0.join("target/debug/deps/old.rlib"))
                .is_some()
        );
    }

    #[test]
    fn relative_context_matches_absolute_cargo_cwd_and_build_references() {
        let mut context = TargetContext::new("target")
            .with_project_root(".")
            .with_build_root("build");
        normalize_context(&mut context).unwrap();
        let cwd = std::env::current_dir().unwrap();
        assert_eq!(context.target_root, cwd.join("target"));
        assert_eq!(context.project_root, Some(cwd.clone()));
        assert_eq!(context.build_root, Some(cwd.join("build")));
        let observation =
            ActiveObservation::complete([
                ObservedCargoProcess::new(CargoTool::Cargo).with_cwd(&cwd)
            ]);
        assert!(active_skip_reason(Some(&context), &observation).is_some());
        let observation =
            ActiveObservation::complete([ObservedCargoProcess::new(CargoTool::Rustc)
                .with_referenced_path(cwd.join("build/deps/libsample.rlib"))]);
        assert!(active_skip_reason(Some(&context), &observation).is_some());
    }

    #[test]
    fn fresh_process_observation_blocks_the_matching_target() {
        let root = std::env::temp_dir().join("reclaim-activity-unit");
        let observation =
            ActiveObservation::complete([
                ObservedCargoProcess::new(CargoTool::Cargo).with_cwd(&root)
            ]);
        let mut activity = guard(observation);
        let reason = activity.skip_reason(&root.join("target/debug/deps/old.rlib"));
        assert!(reason.is_some_and(|reason| reason.contains("observed Cargo or rustc")));
    }

    #[test]
    fn failed_process_inspection_preserves_the_target() {
        let root = std::env::temp_dir().join("reclaim-activity-unit");
        let mut activity = guard(ActiveObservation::failed("procfs unavailable"));
        let reason = activity.skip_reason(&root.join("target/debug/deps/old.rlib"));
        assert!(reason.is_some_and(|reason| reason.contains("procfs unavailable")));
    }

    #[test]
    fn an_unrelated_process_does_not_block_the_target() {
        let root = std::env::temp_dir().join("reclaim-activity-unit");
        let observation =
            ActiveObservation::complete([ObservedCargoProcess::new(CargoTool::Cargo)
                .with_cwd(std::env::temp_dir().join("other-project"))]);
        let mut activity = guard(observation);
        assert!(
            activity
                .skip_reason(&root.join("target/debug/deps/old.rlib"))
                .is_none()
        );
    }
}
