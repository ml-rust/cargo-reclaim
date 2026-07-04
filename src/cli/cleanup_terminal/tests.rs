use std::collections::VecDeque;
use std::path::PathBuf;

use cargo_reclaim::{PathKind, TargetEvidence};
use crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

use super::super::target_report::TargetListEntry;
use super::*;

struct TestEventReader {
    events: VecDeque<Event>,
}

impl TestEventReader {
    fn new(events: impl IntoIterator<Item = Event>) -> Self {
        Self {
            events: events.into_iter().collect(),
        }
    }
}

impl TerminalEventReader for TestEventReader {
    fn read_event(&mut self) -> Result<Option<Event>, CliError> {
        Ok(self.events.pop_front())
    }
}

#[derive(Default)]
struct EventScript {
    events: Vec<Event>,
}

impl EventScript {
    fn press(mut self, code: KeyCode) -> Self {
        self.events.push(key(code));
        self
    }

    fn release(mut self, code: KeyCode) -> Self {
        self.events.push(key_with_kind(code, KeyEventKind::Release));
        self
    }

    fn focus_gained(mut self) -> Self {
        self.events.push(Event::FocusGained);
        self
    }

    fn space(self) -> Self {
        self.press(KeyCode::Char(' '))
    }

    fn down(self) -> Self {
        self.press(KeyCode::Down)
    }

    fn enter(self) -> Self {
        self.press(KeyCode::Enter)
    }

    fn backspace(self) -> Self {
        self.press(KeyCode::Backspace)
    }

    fn esc(self) -> Self {
        self.press(KeyCode::Esc)
    }

    fn q(self) -> Self {
        self.press(KeyCode::Char('q'))
    }

    fn run(
        self,
        report: &TargetsReport,
        start_options: CleanupAssistantStartOptions,
    ) -> Result<TerminalRun, CliError> {
        let mut state = CleanupAssistantState::with_start_options(start_options)?;
        let mut output = Vec::new();
        let mut events = TestEventReader::new(self.events);
        let selection =
            run_cleanup_terminal_assistant_loop(&mut output, &mut events, report, &mut state)?;
        Ok(TerminalRun {
            selection,
            output: String::from_utf8_lossy(&output).into_owned(),
        })
    }
}

#[derive(Debug)]
struct TerminalRun {
    selection: CleanupAssistantSelection,
    output: String,
}

fn key(code: KeyCode) -> Event {
    key_with_kind(code, KeyEventKind::Press)
}

fn key_with_kind(code: KeyCode, kind: KeyEventKind) -> Event {
    Event::Key(KeyEvent::new_with_kind(code, KeyModifiers::NONE, kind))
}

fn target_selection_options(target_count: usize) -> CleanupAssistantStartOptions {
    CleanupAssistantStartOptions {
        selected: vec![false; target_count],
        first_page: CleanupAssistantPage::Targets,
        minimum_page: CleanupAssistantPage::Targets,
        forced_mode: None,
        forced_action: None,
    }
}

fn selected_mode_options(selected: Vec<bool>) -> CleanupAssistantStartOptions {
    CleanupAssistantStartOptions {
        selected,
        first_page: CleanupAssistantPage::Mode,
        minimum_page: CleanupAssistantPage::Mode,
        forced_mode: None,
        forced_action: None,
    }
}

fn forced_action_options(
    selected: Vec<bool>,
    forced_mode: Option<CleanupAssistantMode>,
    forced_action: Option<CleanupAssistantAction>,
) -> CleanupAssistantStartOptions {
    CleanupAssistantStartOptions {
        selected,
        first_page: CleanupAssistantPage::Action,
        minimum_page: CleanupAssistantPage::Action,
        forced_mode,
        forced_action,
    }
}

fn report_fixture() -> TargetsReport {
    let targets = vec![
        TargetListEntry {
            path: PathBuf::from("/workspace/first/target"),
            size_bytes: 1024,
            path_kind: PathKind::Directory,
            evidence: TargetEvidence::StrongMarker {
                marker: ".rustc_info.json".to_string(),
            },
        },
        TargetListEntry {
            path: PathBuf::from("/workspace/second/target"),
            size_bytes: 2048,
            path_kind: PathKind::Directory,
            evidence: TargetEvidence::ProjectContext {
                project_manifest: PathBuf::from("/workspace/second/Cargo.toml"),
            },
        },
        TargetListEntry {
            path: PathBuf::from("/workspace/third/target"),
            size_bytes: 4096,
            path_kind: PathKind::Directory,
            evidence: TargetEvidence::StrongMarker {
                marker: "CACHEDIR.TAG".to_string(),
            },
        },
    ];
    TargetsReport {
        roots: vec![PathBuf::from("/workspace")],
        config_path: None,
        config_version: None,
        total_size_bytes: targets.iter().map(|target| target.size_bytes).sum(),
        targets,
        skipped_paths: Vec::new(),
        problems: Vec::new(),
    }
}

#[test]
fn multi_selects_targets_with_space_and_down() -> Result<(), CliError> {
    let report = report_fixture();

    let run = EventScript::default()
        .space()
        .down()
        .space()
        .enter()
        .enter()
        .enter()
        .run(&report, target_selection_options(report.targets.len()))?;

    assert_eq!(
        run.selection.targets,
        vec![
            PathBuf::from("/workspace/first/target"),
            PathBuf::from("/workspace/second/target")
        ]
    );
    assert_eq!(run.selection.mode, CleanupAssistantMode::SmartTrim);
    assert_eq!(run.selection.action, CleanupAssistantAction::ValidateOnly);
    assert!(run.selection.target_selection_modified);
    assert!(run.output.contains("Select target directories"));
    assert!(run.output.contains("Choose cleanup mode"));
    assert!(run.output.contains("Choose apply decision"));
    Ok(())
}

#[test]
fn validation_only_is_default_action() -> Result<(), CliError> {
    let report = report_fixture();

    let run = EventScript::default()
        .enter()
        .enter()
        .run(&report, selected_mode_options(vec![false, true, false]))?;

    assert_eq!(
        run.selection.targets,
        vec![PathBuf::from("/workspace/second/target")]
    );
    assert_eq!(run.selection.mode, CleanupAssistantMode::SmartTrim);
    assert_eq!(run.selection.action, CleanupAssistantAction::ValidateOnly);
    assert!(!run.selection.target_selection_modified);
    Ok(())
}

#[test]
fn chooses_delete_target_and_execute() -> Result<(), CliError> {
    let report = report_fixture();

    let run = EventScript::default()
        .space()
        .enter()
        .down()
        .enter()
        .down()
        .enter()
        .run(&report, target_selection_options(report.targets.len()))?;

    assert_eq!(
        run.selection.targets,
        vec![PathBuf::from("/workspace/first/target")]
    );
    assert_eq!(run.selection.mode, CleanupAssistantMode::DeleteTarget);
    assert_eq!(run.selection.action, CleanupAssistantAction::Execute);
    assert!(run.selection.target_selection_modified);
    Ok(())
}

#[test]
fn backspace_from_action_returns_to_mode_when_allowed() -> Result<(), CliError> {
    let report = report_fixture();

    let run = EventScript::default()
        .enter()
        .backspace()
        .down()
        .enter()
        .down()
        .enter()
        .run(&report, selected_mode_options(vec![true, false, false]))?;

    assert_eq!(run.selection.mode, CleanupAssistantMode::DeleteTarget);
    assert_eq!(run.selection.action, CleanupAssistantAction::Execute);
    assert!(run.output.contains("Choose cleanup mode"));
    assert!(run.output.contains("Choose apply decision"));
    Ok(())
}

#[test]
fn forced_selector_pages_are_not_reachable_by_backspace() -> Result<(), CliError> {
    let report = report_fixture();

    let target_selector_skipped = EventScript::default()
        .backspace()
        .enter()
        .enter()
        .run(&report, selected_mode_options(vec![false, true, false]))?;

    assert_eq!(
        target_selector_skipped.selection.targets,
        vec![PathBuf::from("/workspace/second/target")]
    );
    assert_eq!(
        target_selector_skipped.selection.mode,
        CleanupAssistantMode::SmartTrim
    );
    assert!(
        !target_selector_skipped
            .output
            .contains("Select target directories")
    );
    assert!(
        target_selector_skipped
            .output
            .contains("Choose cleanup mode")
    );
    assert!(
        target_selector_skipped
            .output
            .contains("Choose apply decision")
    );

    let run = EventScript::default().backspace().enter().run(
        &report,
        forced_action_options(
            vec![true, false, false],
            Some(CleanupAssistantMode::DeleteTarget),
            None,
        ),
    )?;

    assert_eq!(
        run.selection.targets,
        vec![PathBuf::from("/workspace/first/target")]
    );
    assert_eq!(run.selection.mode, CleanupAssistantMode::DeleteTarget);
    assert_eq!(run.selection.action, CleanupAssistantAction::ValidateOnly);
    assert!(!run.output.contains("Select target directories"));
    assert!(!run.output.contains("Choose cleanup mode"));
    assert!(run.output.contains("Choose apply decision"));
    Ok(())
}

#[test]
fn no_target_error_renders_and_recovers_after_keypress() -> Result<(), CliError> {
    let report = report_fixture();

    let run = EventScript::default()
        .enter()
        .focus_gained()
        .release(KeyCode::Char('x'))
        .press(KeyCode::Char('x'))
        .space()
        .enter()
        .enter()
        .enter()
        .run(&report, target_selection_options(report.targets.len()))?;

    assert_eq!(
        run.selection.targets,
        vec![PathBuf::from("/workspace/first/target")]
    );
    assert!(run.output.contains("no targets selected"));
    assert!(run.output.contains("Press any key to continue."));
    assert!(run.output.contains("Choose cleanup mode"));
    Ok(())
}

#[test]
fn esc_cancels() -> Result<(), CliError> {
    let report = report_fixture();

    let run = EventScript::default()
        .esc()
        .run(&report, target_selection_options(report.targets.len()))?;

    assert_eq!(run.selection.action, CleanupAssistantAction::Cancel);
    assert!(run.selection.targets.is_empty());
    Ok(())
}

#[test]
fn q_cancels() -> Result<(), CliError> {
    let report = report_fixture();

    let run = EventScript::default()
        .q()
        .run(&report, target_selection_options(report.targets.len()))?;

    assert_eq!(run.selection.action, CleanupAssistantAction::Cancel);
    assert!(run.selection.targets.is_empty());
    Ok(())
}

#[test]
fn ignores_non_key_and_release_events() -> Result<(), CliError> {
    let report = report_fixture();

    let run = EventScript::default()
        .focus_gained()
        .release(KeyCode::Char(' '))
        .space()
        .enter()
        .enter()
        .enter()
        .run(&report, target_selection_options(report.targets.len()))?;

    assert_eq!(
        run.selection.targets,
        vec![PathBuf::from("/workspace/first/target")]
    );
    assert_eq!(run.selection.action, CleanupAssistantAction::ValidateOnly);
    Ok(())
}

#[test]
fn exhausted_input_returns_usage_error() {
    let report = report_fixture();

    let error = EventScript::default()
        .run(&report, target_selection_options(report.targets.len()))
        .unwrap_err();

    assert!(matches!(error, CliError::Usage(_)));
    assert!(
        error
            .to_string()
            .contains("cleanup assistant input ended before selection completed")
    );
}
