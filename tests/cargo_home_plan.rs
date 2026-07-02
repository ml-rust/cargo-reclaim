use std::path::PathBuf;

use cargo_reclaim::{
    CargoHomeClass, CargoHomeEntry, CargoHomeInput, CargoHomePathKind, CargoHomePlanAction,
    CargoHomeProblem, CargoHomeReport, CargoHomeSource, CargoHomeTotals, PolicyKind,
    build_cargo_home_plan_from_report,
};

#[test]
fn cargo_home_plan_policy_matrix_selects_known_cache_classes() {
    let classes = known_cache_classes();

    let expectations = [
        (PolicyKind::Observe, Vec::new()),
        (
            PolicyKind::Conservative,
            vec![CargoHomeClass::RegistryCache],
        ),
        (
            PolicyKind::Balanced,
            vec![
                CargoHomeClass::RegistryCache,
                CargoHomeClass::RegistrySource,
            ],
        ),
        (PolicyKind::Aggressive, classes.clone()),
        (PolicyKind::Custom, Vec::new()),
    ];

    for (policy, expected_classes) in expectations {
        let plan = build_cargo_home_plan_from_report(report_for_classes(&classes), policy);
        let selected = plan
            .entries
            .iter()
            .filter(|entry| entry.action == CargoHomePlanAction::DeleteCandidate)
            .map(|entry| entry.class)
            .collect::<Vec<_>>();
        assert_eq!(selected, expected_classes);
    }
}

#[test]
fn cargo_home_plan_preserves_sensitive_and_user_authored_classes() {
    let classes = vec![
        CargoHomeClass::Config,
        CargoHomeClass::Credentials,
        CargoHomeClass::InstalledBinaries,
        CargoHomeClass::InstallMetadata,
        CargoHomeClass::UnknownUserAuthored,
    ];

    for policy in [
        PolicyKind::Observe,
        PolicyKind::Conservative,
        PolicyKind::Balanced,
        PolicyKind::Aggressive,
        PolicyKind::Custom,
    ] {
        let plan = build_cargo_home_plan_from_report(report_for_classes(&classes), policy);
        assert!(
            plan.entries
                .iter()
                .all(|entry| entry.action == CargoHomePlanAction::Preserve)
        );
    }
}

#[test]
fn cargo_home_plan_skipped_symlink_and_problem_entries_are_not_delete_candidates() {
    let root = PathBuf::from("/cargo-home");
    let entries = vec![
        entry(&root, "registry/cache", CargoHomeClass::RegistryCache, 10),
        CargoHomeEntry {
            skipped: true,
            reason: "preserved; skipped".to_string(),
            ..entry(&root, "registry/src", CargoHomeClass::RegistrySource, 20)
        },
        CargoHomeEntry {
            path_kind: CargoHomePathKind::Symlink,
            reason: "preserved; symlink".to_string(),
            ..entry(&root, "git/db", CargoHomeClass::GitDatabase, 30)
        },
    ];
    let report = report_with_entries_and_problems(
        entries,
        vec![CargoHomeProblem {
            path: root.join("registry/cache/child"),
            message: "permission denied".to_string(),
        }],
    );

    let plan = build_cargo_home_plan_from_report(report, PolicyKind::Aggressive);

    assert!(
        plan.entries
            .iter()
            .all(|entry| entry.action == CargoHomePlanAction::SkipProblem)
    );
    assert_eq!(plan.totals.delete_candidate_count, 0);
    assert_eq!(plan.totals.skipped_count, 3);
    assert_eq!(plan.totals.problem_count, 1);
}

#[test]
fn cargo_home_plan_delete_bytes_are_cache_subset_reasons_are_set_and_order_is_preserved() {
    let classes = vec![
        CargoHomeClass::Config,
        CargoHomeClass::GitDatabase,
        CargoHomeClass::RegistryCache,
        CargoHomeClass::UnknownUserAuthored,
    ];

    let plan =
        build_cargo_home_plan_from_report(report_for_classes(&classes), PolicyKind::Aggressive);
    let relative_paths = plan
        .entries
        .iter()
        .map(|entry| entry.relative_path.display().to_string())
        .collect::<Vec<_>>();
    assert_eq!(
        relative_paths,
        vec!["config.toml", "git/db", "registry/cache", "custom"]
    );
    assert!(plan.entries.iter().all(|entry| !entry.reason.is_empty()));
    assert_eq!(plan.totals.delete_candidate_count, 2);
    assert_eq!(plan.totals.delete_candidate_bytes, 50);
    assert!(plan.totals.delete_candidate_bytes <= known_cache_bytes(&plan.entries));
}

fn known_cache_classes() -> Vec<CargoHomeClass> {
    vec![
        CargoHomeClass::RegistryIndex,
        CargoHomeClass::RegistryCache,
        CargoHomeClass::RegistrySource,
        CargoHomeClass::GitDatabase,
        CargoHomeClass::GitCheckouts,
    ]
}

fn report_for_classes(classes: &[CargoHomeClass]) -> CargoHomeReport {
    let root = PathBuf::from("/cargo-home");
    let entries = classes
        .iter()
        .enumerate()
        .map(|(index, class)| {
            entry(
                &root,
                relative_path_for_class(*class),
                *class,
                ((index + 1) * 10) as u64,
            )
        })
        .collect();
    report_with_entries_and_problems(entries, Vec::new())
}

fn report_with_entries_and_problems(
    entries: Vec<CargoHomeEntry>,
    problems: Vec<CargoHomeProblem>,
) -> CargoHomeReport {
    CargoHomeReport {
        schema_version: 1,
        input: CargoHomeInput {
            root: PathBuf::from("/cargo-home"),
            source: CargoHomeSource::Explicit,
        },
        totals: CargoHomeTotals {
            entry_count: entries.len(),
            total_bytes: entries.iter().map(|entry| entry.size_bytes).sum(),
            cache_bytes: known_cache_bytes(&entries),
            preserved_bytes: entries.iter().map(|entry| entry.size_bytes).sum(),
            skipped_count: entries.iter().filter(|entry| entry.skipped).count(),
            problem_count: problems.len(),
            known_cache_entry_count: entries
                .iter()
                .filter(|entry| is_known_cache_class(entry.class))
                .count(),
        },
        entries,
        recommendations: Vec::new(),
        problems,
    }
}

fn entry(
    root: &std::path::Path,
    relative_path: impl Into<PathBuf>,
    class: CargoHomeClass,
    size_bytes: u64,
) -> CargoHomeEntry {
    let relative_path = relative_path.into();
    CargoHomeEntry {
        path: root.join(&relative_path),
        relative_path,
        class,
        path_kind: CargoHomePathKind::Directory,
        size_bytes,
        preserved: true,
        skipped: false,
        reason: "preserved; report fixture".to_string(),
    }
}

fn relative_path_for_class(class: CargoHomeClass) -> &'static str {
    match class {
        CargoHomeClass::RegistryIndex => "registry/index",
        CargoHomeClass::RegistryCache => "registry/cache",
        CargoHomeClass::RegistrySource => "registry/src",
        CargoHomeClass::GitDatabase => "git/db",
        CargoHomeClass::GitCheckouts => "git/checkouts",
        CargoHomeClass::Config => "config.toml",
        CargoHomeClass::Credentials => "credentials.toml",
        CargoHomeClass::InstalledBinaries => "bin",
        CargoHomeClass::InstallMetadata => ".crates.toml",
        CargoHomeClass::UnknownUserAuthored => "custom",
    }
}

fn known_cache_bytes(entries: &[impl EntryClassAndSize]) -> u64 {
    entries
        .iter()
        .filter(|entry| is_known_cache_class(entry.class()))
        .map(EntryClassAndSize::size_bytes)
        .sum()
}

trait EntryClassAndSize {
    fn class(&self) -> CargoHomeClass;
    fn size_bytes(&self) -> u64;
}

impl EntryClassAndSize for CargoHomeEntry {
    fn class(&self) -> CargoHomeClass {
        self.class
    }

    fn size_bytes(&self) -> u64 {
        self.size_bytes
    }
}

impl EntryClassAndSize for cargo_reclaim::CargoHomePlanEntry {
    fn class(&self) -> CargoHomeClass {
        self.class
    }

    fn size_bytes(&self) -> u64 {
        self.size_bytes
    }
}

fn is_known_cache_class(class: CargoHomeClass) -> bool {
    matches!(
        class,
        CargoHomeClass::RegistryIndex
            | CargoHomeClass::RegistryCache
            | CargoHomeClass::RegistrySource
            | CargoHomeClass::GitDatabase
            | CargoHomeClass::GitCheckouts
    )
}
