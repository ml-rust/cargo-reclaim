use std::io::{self, Write};

use crossterm::cursor::{Hide, MoveTo, Show};
use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use crossterm::execute;
use crossterm::terminal::{
    self, Clear, ClearType, EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode,
    enable_raw_mode,
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
        draw(writer, report, state, terminal_size())?;
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
        if state.page() == CleanupAssistantPage::Done {
            return Ok(state.selection(report));
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
    size: TerminalSize,
) -> Result<(), CliError> {
    execute!(writer, MoveTo(0, 0), Clear(ClearType::All))?;
    write_line(writer, size.width, "cargo-reclaim cleanup assistant")?;
    write_line(writer, size.width, "")?;
    match state.page() {
        CleanupAssistantPage::Targets => draw_targets(writer, report, state, size)?,
        CleanupAssistantPage::Mode => draw_mode(writer, state, size)?,
        CleanupAssistantPage::Action => draw_action(writer, state, size)?,
        CleanupAssistantPage::Done => {}
    }
    write_line(writer, size.width, "")?;
    write_line(
        writer,
        size.width,
        "Enter: continue/select  Space: toggle  Up/Down: move  Backspace: back  Esc/q: cancel",
    )?;
    if state.page() == CleanupAssistantPage::Targets {
        write_line(writer, size.width, "a: select all  n: select none")?;
    }
    writer.flush()?;
    Ok(())
}

#[derive(Debug, Clone, Copy)]
struct TerminalSize {
    width: usize,
    height: usize,
}

impl TerminalSize {
    fn new(width: u16, height: u16) -> Self {
        Self {
            width: usize::from(width).max(20),
            height: usize::from(height).max(8),
        }
    }
}

fn terminal_size() -> TerminalSize {
    let (width, height) = terminal::size().unwrap_or((120, 30));
    TerminalSize::new(width, height)
}

fn write_line(writer: &mut impl Write, width: usize, text: &str) -> Result<(), CliError> {
    let line = truncate_to_width(text, width);
    write!(writer, "{line}\r\n")?;
    Ok(())
}

fn truncate_to_width(text: &str, width: usize) -> String {
    let width = width.max(1);
    let count = text.chars().count();
    if count <= width {
        return text.to_string();
    }
    if width == 1 {
        return "…".to_string();
    }
    let mut truncated = text.chars().take(width - 1).collect::<String>();
    truncated.push('…');
    truncated
}

fn target_row_budget(size: TerminalSize) -> usize {
    // Header(2) + target heading(3) + footer(3).
    size.height.saturating_sub(8).max(1)
}

fn visible_range(total: usize, cursor: usize, limit: usize) -> std::ops::Range<usize> {
    if total <= limit {
        return 0..total;
    }
    let half = limit / 2;
    let mut start = cursor.saturating_sub(half);
    start = start.min(total - limit);
    start..start + limit
}

fn target_row_text(
    cursor: bool,
    selected: bool,
    size: String,
    evidence: &str,
    path: String,
) -> String {
    let cursor = if cursor { ">" } else { " " };
    let selected = if selected { "[x]" } else { "[ ]" };
    format!("{cursor} {selected} {size:>10} {evidence:<18} {path}")
}

fn draw_targets(
    writer: &mut impl Write,
    report: &TargetsReport,
    state: &CleanupAssistantState,
    size: TerminalSize,
) -> Result<(), CliError> {
    let limit = target_row_budget(size);
    let range = visible_range(report.targets.len(), state.cursor(), limit);
    let targets_label = if range.len() < report.targets.len() {
        format!(
            "Targets: {} ({}) | showing {}-{}",
            report.targets.len(),
            human_bytes(report.total_size_bytes),
            range.start + 1,
            range.end
        )
    } else {
        format!(
            "Targets: {} ({})",
            report.targets.len(),
            human_bytes(report.total_size_bytes)
        )
    };

    write_line(writer, size.width, "Select target directories")?;
    write_line(writer, size.width, &targets_label)?;
    write_line(writer, size.width, "")?;
    for index in range {
        let target = &report.targets[index];
        let text = target_row_text(
            index == state.cursor(),
            state.selected()[index],
            human_bytes(target.size_bytes),
            evidence_label(&target.evidence),
            target.path.display().to_string(),
        );
        write_line(writer, size.width, &text)?;
    }
    Ok(())
}

fn draw_mode(
    writer: &mut impl Write,
    state: &CleanupAssistantState,
    size: TerminalSize,
) -> Result<(), CliError> {
    write_line(writer, size.width, "Choose cleanup mode")?;
    write_line(writer, size.width, "")?;
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
        write_line(
            writer,
            size.width,
            &format!("{cursor} {label:<34} {detail} {selected}"),
        )?;
    }
    Ok(())
}

fn draw_action(
    writer: &mut impl Write,
    state: &CleanupAssistantState,
    size: TerminalSize,
) -> Result<(), CliError> {
    write_line(writer, size.width, "Choose apply decision")?;
    write_line(writer, size.width, "")?;
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
        write_line(writer, size.width, &format!("{cursor} {label} {selected}"))?;
    }
    Ok(())
}

fn draw_error(writer: &mut impl Write, error: &CliError) -> Result<(), CliError> {
    let size = terminal_size();
    write_line(writer, size.width, "")?;
    write_line(writer, size.width, &error.to_string())?;
    write_line(writer, size.width, "Press any key to continue.")?;
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
