use std::path::PathBuf;

use super::CliError;
use super::target_report::TargetsReport;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum CleanupAssistantMode {
    SmartTrim,
    DeleteTarget,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum CleanupAssistantAction {
    ValidateOnly,
    Execute,
    Cancel,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct CleanupAssistantSelection {
    pub(super) targets: Vec<PathBuf>,
    pub(super) mode: CleanupAssistantMode,
    pub(super) action: CleanupAssistantAction,
    pub(super) target_selection_modified: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum CleanupAssistantPage {
    Targets,
    Mode,
    Action,
    Done,
}

#[derive(Debug, Clone)]
pub(super) struct CleanupAssistantStartOptions {
    pub(super) selected: Vec<bool>,
    pub(super) first_page: CleanupAssistantPage,
    pub(super) minimum_page: CleanupAssistantPage,
    pub(super) forced_mode: Option<CleanupAssistantMode>,
    pub(super) forced_action: Option<CleanupAssistantAction>,
}

impl CleanupAssistantStartOptions {
    #[cfg(test)]
    pub(super) fn target_selection(target_count: usize) -> Self {
        Self {
            selected: vec![false; target_count],
            first_page: CleanupAssistantPage::Targets,
            minimum_page: CleanupAssistantPage::Targets,
            forced_mode: None,
            forced_action: None,
        }
    }
}

#[derive(Debug, Clone)]
pub(super) struct CleanupAssistantState {
    page: CleanupAssistantPage,
    minimum_page: CleanupAssistantPage,
    cursor: usize,
    selected: Vec<bool>,
    target_selection_modified: bool,
    mode: CleanupAssistantMode,
    action: CleanupAssistantAction,
    forced_mode: Option<CleanupAssistantMode>,
    forced_action: Option<CleanupAssistantAction>,
}

impl CleanupAssistantState {
    #[cfg(test)]
    pub(super) fn new(
        target_count: usize,
        forced_mode: Option<CleanupAssistantMode>,
        forced_action: Option<CleanupAssistantAction>,
    ) -> Result<Self, CliError> {
        let mut options = CleanupAssistantStartOptions::target_selection(target_count);
        options.forced_mode = forced_mode;
        options.forced_action = forced_action;
        Self::with_start_options(options)
    }

    pub(super) fn with_start_options(
        options: CleanupAssistantStartOptions,
    ) -> Result<Self, CliError> {
        let target_count = options.selected.len();
        if target_count == 0 {
            return Err(CliError::Usage(
                "cleanup found no target directories to select".to_string(),
            ));
        }
        let cursor = match options.first_page {
            CleanupAssistantPage::Targets | CleanupAssistantPage::Done => 0,
            CleanupAssistantPage::Mode => mode_cursor(
                options
                    .forced_mode
                    .unwrap_or(CleanupAssistantMode::SmartTrim),
            ),
            CleanupAssistantPage::Action => action_cursor(
                options
                    .forced_action
                    .unwrap_or(CleanupAssistantAction::ValidateOnly),
            ),
        };
        Ok(Self {
            page: options.first_page,
            minimum_page: options.minimum_page,
            cursor,
            selected: options.selected,
            target_selection_modified: false,
            mode: options
                .forced_mode
                .unwrap_or(CleanupAssistantMode::SmartTrim),
            action: options
                .forced_action
                .unwrap_or(CleanupAssistantAction::ValidateOnly),
            forced_mode: options.forced_mode,
            forced_action: options.forced_action,
        })
    }

    pub(super) fn page(&self) -> CleanupAssistantPage {
        self.page
    }

    pub(super) fn cursor(&self) -> usize {
        self.cursor
    }

    pub(super) fn selected(&self) -> &[bool] {
        &self.selected
    }

    pub(super) fn mode(&self) -> CleanupAssistantMode {
        self.mode
    }

    pub(super) fn action(&self) -> CleanupAssistantAction {
        self.action
    }

    pub(super) fn move_up(&mut self) {
        if self.cursor > 0 {
            self.cursor -= 1;
        }
    }

    pub(super) fn move_down(&mut self) {
        let max = self.option_count().saturating_sub(1);
        if self.cursor < max {
            self.cursor += 1;
        }
    }

    pub(super) fn toggle_current_target(&mut self) {
        if self.page != CleanupAssistantPage::Targets {
            return;
        }
        if let Some(selected) = self.selected.get_mut(self.cursor) {
            *selected = !*selected;
            self.target_selection_modified = true;
        }
    }

    pub(super) fn select_all_targets(&mut self) {
        if self.page == CleanupAssistantPage::Targets {
            self.selected.fill(true);
            self.target_selection_modified = true;
        }
    }

    pub(super) fn select_no_targets(&mut self) {
        if self.page == CleanupAssistantPage::Targets {
            self.selected.fill(false);
            self.target_selection_modified = true;
        }
    }

    pub(super) fn choose_current(&mut self) {
        match self.page {
            CleanupAssistantPage::Targets => self.toggle_current_target(),
            CleanupAssistantPage::Mode => {
                self.mode = if self.cursor == 0 {
                    CleanupAssistantMode::SmartTrim
                } else {
                    CleanupAssistantMode::DeleteTarget
                };
            }
            CleanupAssistantPage::Action => {
                self.action = match self.cursor {
                    0 => CleanupAssistantAction::ValidateOnly,
                    1 => CleanupAssistantAction::Execute,
                    _ => CleanupAssistantAction::Cancel,
                };
            }
            CleanupAssistantPage::Done => {}
        }
    }

    pub(super) fn next_page(&mut self) -> Result<(), CliError> {
        match self.page {
            CleanupAssistantPage::Targets => {
                if !self.selected.iter().any(|selected| *selected) {
                    return Err(CliError::Usage("no targets selected".to_string()));
                }
                if self.forced_mode.is_some() && self.forced_action.is_some() {
                    self.page = CleanupAssistantPage::Done;
                    self.cursor = 0;
                } else if self.forced_mode.is_some() {
                    self.page = CleanupAssistantPage::Action;
                    self.cursor = action_cursor(self.action);
                } else {
                    self.page = CleanupAssistantPage::Mode;
                    self.cursor = mode_cursor(self.mode);
                }
            }
            CleanupAssistantPage::Mode => {
                if self.forced_action.is_some() {
                    self.page = CleanupAssistantPage::Done;
                    self.cursor = 0;
                } else {
                    self.page = CleanupAssistantPage::Action;
                    self.cursor = action_cursor(self.action);
                }
            }
            CleanupAssistantPage::Action => {
                self.page = CleanupAssistantPage::Done;
                self.cursor = 0;
            }
            CleanupAssistantPage::Done => {}
        }
        Ok(())
    }

    pub(super) fn previous_page(&mut self) {
        match self.page {
            CleanupAssistantPage::Targets => {}
            CleanupAssistantPage::Mode => {
                self.set_page_if_allowed(CleanupAssistantPage::Targets);
            }
            CleanupAssistantPage::Action => {
                if self.forced_mode.is_some() {
                    self.set_page_if_allowed(CleanupAssistantPage::Targets);
                } else {
                    self.set_page_if_allowed(CleanupAssistantPage::Mode);
                }
            }
            CleanupAssistantPage::Done => {
                if self.forced_action.is_some() && self.forced_mode.is_some() {
                    self.set_page_if_allowed(CleanupAssistantPage::Targets);
                } else if self.forced_action.is_some() {
                    self.set_page_if_allowed(CleanupAssistantPage::Mode);
                } else {
                    self.set_page_if_allowed(CleanupAssistantPage::Action);
                }
            }
        }
    }

    pub(super) fn cancel(&mut self) {
        self.page = CleanupAssistantPage::Done;
        self.action = CleanupAssistantAction::Cancel;
        self.cursor = 0;
    }

    pub(super) fn selection(&self, report: &TargetsReport) -> CleanupAssistantSelection {
        let targets = report
            .targets
            .iter()
            .zip(&self.selected)
            .filter_map(|(target, selected)| selected.then_some(target.path.clone()))
            .collect();
        CleanupAssistantSelection {
            targets,
            mode: self.mode,
            action: self.action,
            target_selection_modified: self.target_selection_modified,
        }
    }

    fn option_count(&self) -> usize {
        match self.page {
            CleanupAssistantPage::Targets => self.selected.len(),
            CleanupAssistantPage::Mode => 2,
            CleanupAssistantPage::Action => 3,
            CleanupAssistantPage::Done => 1,
        }
    }

    fn set_page_if_allowed(&mut self, page: CleanupAssistantPage) {
        if page_rank(page) < page_rank(self.minimum_page) {
            return;
        }
        self.page = page;
        self.cursor = match page {
            CleanupAssistantPage::Targets | CleanupAssistantPage::Done => 0,
            CleanupAssistantPage::Mode => mode_cursor(self.mode),
            CleanupAssistantPage::Action => action_cursor(self.action),
        };
    }
}

fn page_rank(page: CleanupAssistantPage) -> usize {
    match page {
        CleanupAssistantPage::Targets => 0,
        CleanupAssistantPage::Mode => 1,
        CleanupAssistantPage::Action => 2,
        CleanupAssistantPage::Done => 3,
    }
}

fn mode_cursor(mode: CleanupAssistantMode) -> usize {
    match mode {
        CleanupAssistantMode::SmartTrim => 0,
        CleanupAssistantMode::DeleteTarget => 1,
    }
}

fn action_cursor(action: CleanupAssistantAction) -> usize {
    match action {
        CleanupAssistantAction::ValidateOnly => 0,
        CleanupAssistantAction::Execute => 1,
        CleanupAssistantAction::Cancel => 2,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn toggles_target_selection() -> Result<(), CliError> {
        let mut state = CleanupAssistantState::new(2, None, None)?;
        assert_eq!(state.selected(), &[false, false]);

        state.toggle_current_target();
        state.move_down();
        state.toggle_current_target();

        assert_eq!(state.selected(), &[true, true]);
        Ok(())
    }

    #[test]
    fn requires_at_least_one_target_before_mode_page() -> Result<(), CliError> {
        let mut state = CleanupAssistantState::new(1, None, None)?;
        let error = state.next_page().unwrap_err();

        assert!(error.to_string().contains("no targets selected"));
        assert_eq!(state.page(), CleanupAssistantPage::Targets);
        Ok(())
    }

    #[test]
    fn defaults_to_smart_trim_and_validate_only() -> Result<(), CliError> {
        let mut state = CleanupAssistantState::new(1, None, None)?;
        state.toggle_current_target();
        state.next_page()?;
        state.next_page()?;

        assert_eq!(state.mode(), CleanupAssistantMode::SmartTrim);
        assert_eq!(state.action(), CleanupAssistantAction::ValidateOnly);
        Ok(())
    }

    #[test]
    fn chooses_delete_mode_and_execute_action() -> Result<(), CliError> {
        let mut state = CleanupAssistantState::new(1, None, None)?;
        state.toggle_current_target();
        state.next_page()?;
        state.move_down();
        state.choose_current();
        state.next_page()?;
        state.move_down();
        state.choose_current();

        assert_eq!(state.mode(), CleanupAssistantMode::DeleteTarget);
        assert_eq!(state.action(), CleanupAssistantAction::Execute);
        Ok(())
    }

    #[test]
    fn cancel_marks_done_without_apply_action() -> Result<(), CliError> {
        let mut state = CleanupAssistantState::new(1, None, None)?;
        state.cancel();

        assert_eq!(state.page(), CleanupAssistantPage::Done);
        assert_eq!(state.action(), CleanupAssistantAction::Cancel);
        Ok(())
    }

    #[test]
    fn no_targets_is_usage_error() {
        let error = CleanupAssistantState::new(0, None, None).unwrap_err();

        assert!(
            error
                .to_string()
                .contains("cleanup found no target directories")
        );
    }

    #[test]
    fn select_all_and_none_update_target_page() -> Result<(), CliError> {
        let mut state = CleanupAssistantState::new(3, None, None)?;

        state.select_all_targets();
        assert_eq!(state.selected(), &[true, true, true]);

        state.select_no_targets();
        assert_eq!(state.selected(), &[false, false, false]);
        Ok(())
    }

    #[test]
    fn forced_mode_skips_mode_page() -> Result<(), CliError> {
        let mut state =
            CleanupAssistantState::new(1, Some(CleanupAssistantMode::DeleteTarget), None)?;
        state.toggle_current_target();
        state.next_page()?;

        assert_eq!(state.page(), CleanupAssistantPage::Action);
        assert_eq!(state.mode(), CleanupAssistantMode::DeleteTarget);
        Ok(())
    }

    #[test]
    fn forced_action_skips_action_page() -> Result<(), CliError> {
        let mut state = CleanupAssistantState::new(1, None, Some(CleanupAssistantAction::Execute))?;
        state.toggle_current_target();
        state.next_page()?;
        state.next_page()?;

        assert_eq!(state.page(), CleanupAssistantPage::Done);
        assert_eq!(state.action(), CleanupAssistantAction::Execute);
        Ok(())
    }

    #[test]
    fn started_at_mode_cannot_back_into_target_page() -> Result<(), CliError> {
        let mut state = CleanupAssistantState::with_start_options(CleanupAssistantStartOptions {
            selected: vec![true],
            first_page: CleanupAssistantPage::Mode,
            minimum_page: CleanupAssistantPage::Mode,
            forced_mode: None,
            forced_action: None,
        })?;

        state.previous_page();

        assert_eq!(state.page(), CleanupAssistantPage::Mode);
        Ok(())
    }

    #[test]
    fn started_at_action_cannot_back_into_mode_or_target_pages() -> Result<(), CliError> {
        let mut state = CleanupAssistantState::with_start_options(CleanupAssistantStartOptions {
            selected: vec![true],
            first_page: CleanupAssistantPage::Action,
            minimum_page: CleanupAssistantPage::Action,
            forced_mode: Some(CleanupAssistantMode::DeleteTarget),
            forced_action: None,
        })?;

        state.previous_page();

        assert_eq!(state.page(), CleanupAssistantPage::Action);
        assert_eq!(state.mode(), CleanupAssistantMode::DeleteTarget);
        Ok(())
    }
}
