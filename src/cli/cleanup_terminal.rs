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
    CleanupAssistantStartOptions, CleanupAssistantState,
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
    start_options: CleanupAssistantStartOptions,
) -> Result<CleanupAssistantSelection, CliError> {
    let mut state = CleanupAssistantState::with_start_options(start_options)?;
    let mut stderr = io::stderr();
    let _guard = TerminalGuard::enter(&mut stderr)?;
    let mut events = CrosstermEventReader;

    run_cleanup_terminal_assistant_loop(&mut stderr, &mut events, report, &mut state)
}

fn run_cleanup_terminal_assistant_loop(
    writer: &mut impl Write,
    events: &mut impl TerminalEventReader,
    report: &TargetsReport,
    state: &mut CleanupAssistantState,
) -> Result<CleanupAssistantSelection, CliError> {
    loop {
        draw(writer, report, state)?;
        if state.page() == CleanupAssistantPage::Done {
            return Ok(state.selection(report));
        }

        let Some(event) = events.read_event()? else {
            return Err(CliError::Usage(
                "cleanup assistant input ended before selection completed".to_string(),
            ));
        };
        let Event::Key(key) = event else {
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
                    draw_error(writer, &error)?;
                    wait_for_keypress(events)?;
                }
            }
            KeyCode::Backspace | KeyCode::Left => state.previous_page(),
            _ => {}
        }
    }
}

trait TerminalEventReader {
    fn read_event(&mut self) -> Result<Option<Event>, CliError>;
}

struct CrosstermEventReader;

impl TerminalEventReader for CrosstermEventReader {
    fn read_event(&mut self) -> Result<Option<Event>, CliError> {
        Ok(Some(event::read()?))
    }
}

fn draw(
    writer: &mut impl Write,
    report: &TargetsReport,
    state: &CleanupAssistantState,
) -> Result<(), CliError> {
    execute!(writer, MoveTo(0, 0), Clear(ClearType::All))?;
    writeln!(writer, "cargo-reclaim cleanup assistant")?;
    writeln!(writer)?;
    match state.page() {
        CleanupAssistantPage::Targets => draw_targets(writer, report, state)?,
        CleanupAssistantPage::Mode => draw_mode(writer, state)?,
        CleanupAssistantPage::Action => draw_action(writer, state)?,
        CleanupAssistantPage::Done => {}
    }
    writeln!(writer)?;
    writeln!(
        writer,
        "Enter: continue/select  Space: toggle  Up/Down: move  Backspace: back  Esc/q: cancel"
    )?;
    if state.page() == CleanupAssistantPage::Targets {
        writeln!(writer, "a: select all  n: select none")?;
    }
    writer.flush()?;
    Ok(())
}

fn draw_targets(
    writer: &mut impl Write,
    report: &TargetsReport,
    state: &CleanupAssistantState,
) -> Result<(), CliError> {
    writeln!(writer, "Select target directories")?;
    writeln!(
        writer,
        "Targets: {} ({})",
        report.targets.len(),
        human_bytes(report.total_size_bytes)
    )?;
    writeln!(writer)?;
    for (index, target) in report.targets.iter().enumerate() {
        let cursor = if index == state.cursor() { ">" } else { " " };
        let selected = if state.selected()[index] {
            "[x]"
        } else {
            "[ ]"
        };
        writeln!(
            writer,
            "{cursor} {selected} {:>10} {:<18} {}",
            human_bytes(target.size_bytes),
            evidence_label(&target.evidence),
            target.path.display()
        )?;
    }
    Ok(())
}

fn draw_mode(writer: &mut impl Write, state: &CleanupAssistantState) -> Result<(), CliError> {
    writeln!(writer, "Choose cleanup mode")?;
    writeln!(writer)?;
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
        writeln!(writer, "{cursor} {label:<34} {detail} {selected}")?;
    }
    Ok(())
}

fn draw_action(writer: &mut impl Write, state: &CleanupAssistantState) -> Result<(), CliError> {
    writeln!(writer, "Choose apply decision")?;
    writeln!(writer)?;
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
        writeln!(writer, "{cursor} {label} {selected}")?;
    }
    Ok(())
}

fn draw_error(writer: &mut impl Write, error: &CliError) -> Result<(), CliError> {
    writeln!(writer)?;
    writeln!(writer, "{error}")?;
    writeln!(writer, "Press any key to continue.")?;
    writer.flush()?;
    Ok(())
}

fn wait_for_keypress(events: &mut impl TerminalEventReader) -> Result<(), CliError> {
    loop {
        let Some(event) = events.read_event()? else {
            return Err(CliError::Usage(
                "cleanup assistant input ended while waiting for keypress".to_string(),
            ));
        };
        if matches!(
            event,
            Event::Key(key) if key.kind == KeyEventKind::Press
        ) {
            return Ok(());
        }
    }
}

#[cfg(test)]
mod tests;
