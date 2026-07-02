use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActiveObservation {
    pub process_view: ProcessView,
}

impl ActiveObservation {
    pub fn not_attempted() -> Self {
        Self {
            process_view: ProcessView::NotAttempted,
        }
    }

    pub fn permission_limited(reason: impl Into<String>) -> Self {
        Self {
            process_view: ProcessView::PermissionLimited {
                reason: reason.into(),
            },
        }
    }

    pub fn failed(reason: impl Into<String>) -> Self {
        Self {
            process_view: ProcessView::Failed {
                reason: reason.into(),
            },
        }
    }

    pub fn complete(processes: impl IntoIterator<Item = ObservedCargoProcess>) -> Self {
        Self {
            process_view: ProcessView::Complete {
                processes: processes.into_iter().collect(),
            },
        }
    }
}

impl Default for ActiveObservation {
    fn default() -> Self {
        Self::not_attempted()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProcessView {
    NotAttempted,
    PermissionLimited {
        reason: String,
    },
    Failed {
        reason: String,
    },
    Complete {
        processes: Vec<ObservedCargoProcess>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObservedCargoProcess {
    pub tool: CargoTool,
    pub cwd: Option<PathBuf>,
    pub referenced_paths: Vec<PathBuf>,
}

impl ObservedCargoProcess {
    pub fn new(tool: CargoTool) -> Self {
        Self {
            tool,
            cwd: None,
            referenced_paths: Vec::new(),
        }
    }

    pub fn with_cwd(mut self, cwd: impl Into<PathBuf>) -> Self {
        self.cwd = Some(cwd.into());
        self
    }

    pub fn with_referenced_path(mut self, path: impl Into<PathBuf>) -> Self {
        self.referenced_paths.push(path.into());
        self
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CargoTool {
    Cargo,
    Rustc,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TargetContext {
    pub project_root: Option<PathBuf>,
    pub target_root: PathBuf,
    pub build_root: Option<PathBuf>,
}

impl TargetContext {
    pub fn new(target_root: impl Into<PathBuf>) -> Self {
        Self {
            project_root: None,
            target_root: target_root.into(),
            build_root: None,
        }
    }

    pub fn with_project_root(mut self, project_root: impl Into<PathBuf>) -> Self {
        self.project_root = Some(project_root.into());
        self
    }

    pub fn with_build_root(mut self, build_root: impl Into<PathBuf>) -> Self {
        self.build_root = Some(build_root.into());
        self
    }

    pub(super) fn active_match<'a>(
        &'a self,
        process: &'a ObservedCargoProcess,
    ) -> Option<ActiveMatch<'a>> {
        if let (Some(project_root), Some(cwd)) = (&self.project_root, &process.cwd)
            && path_is_under(cwd, project_root)
        {
            return Some(ActiveMatch::CwdUnderProject { cwd, project_root });
        }

        for referenced_path in &process.referenced_paths {
            if paths_overlap(referenced_path, &self.target_root) {
                return Some(ActiveMatch::ReferencedPathOverlapsRoot {
                    referenced_path,
                    root: &self.target_root,
                });
            }

            if let Some(build_root) = &self.build_root
                && paths_overlap(referenced_path, build_root)
            {
                return Some(ActiveMatch::ReferencedPathOverlapsRoot {
                    referenced_path,
                    root: build_root,
                });
            }
        }

        None
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ActiveMatch<'a> {
    CwdUnderProject {
        cwd: &'a Path,
        project_root: &'a Path,
    },
    ReferencedPathOverlapsRoot {
        referenced_path: &'a Path,
        root: &'a Path,
    },
}

fn path_is_under(path: &Path, root: &Path) -> bool {
    path == root || path.starts_with(root)
}

fn paths_overlap(left: &Path, right: &Path) -> bool {
    left == right || left.starts_with(right) || right.starts_with(left)
}
