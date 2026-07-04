use std::io::{self, Write};

use crossterm::cursor::{Hide, MoveTo, Show};
use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use crossterm::execute;
use crossterm::terminal::{
    Clear, ClearType, EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};

use super::CliError;
use super::cleanup_assistant::{
    CleanupAssistantAction, CleanupAssistantMode, CleanupAssistantPage, CleanupAssistantSelection,
    CleanupAssistantState,
};
use super::target_report::{TargetsReport, evidence_label, human_bytes};

struct TerminalGuard;

impl TerminalGuard {
    fn enter(stderr: &mut io::Stderr) -> Result<Self, CliError> {
        enable_raw_mode()?;
        execute!(stderr, EnterAlternateScreen, Hide)?;
        Ok(Self)
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let mut stderr = io::stderr();
        let _ = execute!(stderr, Show, LeaveAlternateScreen);
    }
}

pub(super) fn run_cleanup_terminal_assistant(
    report: &TargetsReport,
    forced_mode: Option<CleanupAssistantMode>,
    forced_action: Option<CleanupAssistantAction>,
) -> Result<CleanupAssistantSelection, CliError> {
    let mut state = CleanupAssistantState::new(report.targets.len(), forced_mode, forced_action)?;
    let mut stderr = io::stderr();
    let _guard = TerminalGuard::enter(&mut stderr)?;

    loop {
        draw(&mut stderr, report, &state)?;
        if state.page() == CleanupAssistantPage::Done {
            return Ok(state.selection(report));
        }

        let Event::Key(key) = event::read()? else {
            continue;
        };
        if key.kind != KeyEventKind::Press {
            continue;
        }
        match key.code {
            KeyCode::Char('q') | KeyCode::Esc => state.cancel(),
            KeyCode::Up | KeyCode::Char('k') => state.move_up(),
            KeyCode::Down | KeyCode::Char('j') => state.move_down(),
            KeyCode::Char('a') => state.select_all_targets(),
            KeyCode::Char('n') => state.select_no_targets(),
            KeyCode::Char(' ') => state.choose_current(),
            KeyCode::Enter => {
                if state.page() != CleanupAssistantPage::Targets {
                    state.choose_current();
                }
                if let Err(error) = state.next_page() {
                    draw_error(&mut stderr, &error)?;
                    wait_for_keypress()?;
                }
            }
            KeyCode::Backspace | KeyCode::Left => state.previous_page(),
            _ => {}
        }
    }
}

fn draw(
    stderr: &mut io::Stderr,
    report: &TargetsReport,
    state: &CleanupAssistantState,
) -> Result<(), CliError> {
    execute!(stderr, MoveTo(0, 0), Clear(ClearType::All))?;
    writeln!(stderr, "cargo-reclaim cleanup assistant")?;
    writeln!(stderr)?;
    match state.page() {
        CleanupAssistantPage::Targets => draw_targets(stderr, report, state)?,
        CleanupAssistantPage::Mode => draw_mode(stderr, state)?,
        CleanupAssistantPage::Action => draw_action(stderr, state)?,
        CleanupAssistantPage::Done => {}
    }
    writeln!(stderr)?;
    writeln!(
        stderr,
        "Enter: continue/select  Space: toggle  Up/Down: move  Backspace: back  Esc/q: cancel"
    )?;
    if state.page() == CleanupAssistantPage::Targets {
        writeln!(stderr, "a: select all  n: select none")?;
    }
    stderr.flush()?;
    Ok(())
}

fn draw_targets(
    stderr: &mut io::Stderr,
    report: &TargetsReport,
    state: &CleanupAssistantState,
) -> Result<(), CliError> {
    writeln!(stderr, "Select target directories")?;
    writeln!(
        stderr,
        "Targets: {} ({})",
        report.targets.len(),
        human_bytes(report.total_size_bytes)
    )?;
    writeln!(stderr)?;
    for (index, target) in report.targets.iter().enumerate() {
        let cursor = if index == state.cursor() { ">" } else { " " };
        let selected = if state.selected()[index] {
            "[x]"
        } else {
            "[ ]"
        };
        writeln!(
            stderr,
            "{cursor} {selected} {:>10} {:<18} {}",
            human_bytes(target.size_bytes),
            evidence_label(&target.evidence),
            target.path.display()
        )?;
    }
    Ok(())
}

fn draw_mode(stderr: &mut io::Stderr, state: &CleanupAssistantState) -> Result<(), CliError> {
    writeln!(stderr, "Choose cleanup mode")?;
    writeln!(stderr)?;
    let modes = [
        (
            CleanupAssistantMode::SmartTrim,
            "Smart trim selected targets",
            "Planner-selected stale artifacts only",
        ),
        (
            CleanupAssistantMode::DeleteTarget,
            "Delete selected target directories",
            "Whole-target deletion",
        ),
    ];
    for (index, (mode, label, detail)) in modes.iter().enumerate() {
        let cursor = if index == state.cursor() { ">" } else { " " };
        let selected = if *mode == state.mode() {
            "(default)"
        } else {
            ""
        };
        writeln!(stderr, "{cursor} {label:<34} {detail} {selected}")?;
    }
    Ok(())
}

fn draw_action(stderr: &mut io::Stderr, state: &CleanupAssistantState) -> Result<(), CliError> {
    writeln!(stderr, "Choose apply decision")?;
    writeln!(stderr)?;
    let actions = [
        (CleanupAssistantAction::ValidateOnly, "Validate only"),
        (CleanupAssistantAction::Execute, "Execute"),
        (CleanupAssistantAction::Cancel, "Cancel"),
    ];
    for (index, (action, label)) in actions.iter().enumerate() {
        let cursor = if index == state.cursor() { ">" } else { " " };
        let selected = if *action == state.action() {
            "(default)"
        } else {
            ""
        };
        writeln!(stderr, "{cursor} {label} {selected}")?;
    }
    Ok(())
}

fn draw_error(stderr: &mut io::Stderr, error: &CliError) -> Result<(), CliError> {
    writeln!(stderr)?;
    writeln!(stderr, "{error}")?;
    writeln!(stderr, "Press any key to continue.")?;
    stderr.flush()?;
    Ok(())
}

fn wait_for_keypress() -> Result<(), CliError> {
    loop {
        if matches!(event::read()?, Event::Key(_)) {
            return Ok(());
        }
    }
}
