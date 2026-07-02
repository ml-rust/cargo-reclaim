use std::collections::HashSet;
use std::path::{Path, PathBuf};

use crate::planner::{ActiveObservation, TargetContext};

pub trait ActiveObservationProvider {
    fn observe(&self, scope: &ActiveObservationScope) -> ActiveObservation;
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ActiveObservationScope {
    target_contexts: Vec<TargetContext>,
}

impl ActiveObservationScope {
    pub fn from_target_contexts(contexts: impl IntoIterator<Item = TargetContext>) -> Self {
        let mut seen = HashSet::new();
        let mut target_contexts = Vec::new();

        for context in contexts {
            let key = ContextKey::from(&context);
            if seen.insert(key) {
                target_contexts.push(context);
            }
        }

        Self { target_contexts }
    }

    pub fn target_contexts(&self) -> &[TargetContext] {
        &self.target_contexts
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct ContextKey {
    project_root: Option<PathBuf>,
    target_root: PathBuf,
    build_root: Option<PathBuf>,
}

impl From<&TargetContext> for ContextKey {
    fn from(context: &TargetContext) -> Self {
        Self {
            project_root: normalize_optional_path(context.project_root.as_deref()),
            target_root: normalize_path(&context.target_root),
            build_root: normalize_optional_path(context.build_root.as_deref()),
        }
    }
}

fn normalize_optional_path(path: Option<&Path>) -> Option<PathBuf> {
    path.map(normalize_path)
}

fn normalize_path(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();

    for component in path.components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                if !normalized.pop() {
                    normalized.push(component.as_os_str());
                }
            }
            _ => normalized.push(component.as_os_str()),
        }
    }

    normalized
}

#[cfg(test)]
mod tests {
    use crate::planner::TargetContext;

    use super::ActiveObservationScope;

    #[test]
    fn scope_dedupes_equivalent_target_contexts() {
        let scope = ActiveObservationScope::from_target_contexts([
            TargetContext::new("/work/project/target")
                .with_project_root("/work/project")
                .with_build_root("/work/project/build"),
            TargetContext::new("/work/project/./target")
                .with_project_root("/work/project")
                .with_build_root("/work/project/build"),
        ]);

        assert_eq!(scope.target_contexts().len(), 1);
    }
}
