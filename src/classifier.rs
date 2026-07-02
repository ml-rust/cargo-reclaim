use std::ffi::OsStr;
use std::path::{Component, Path};

use crate::model::ArtifactClass;

pub fn classify_target_relative_path(path: impl AsRef<Path>) -> ArtifactClass {
    Classifier.classify_target_relative_path(path)
}

#[derive(Debug, Clone, Copy, Default)]
pub struct Classifier;

impl Classifier {
    pub fn classify_target_relative_path(self, path: impl AsRef<Path>) -> ArtifactClass {
        let Some(components) = normalize_components(path.as_ref()) else {
            return ArtifactClass::Unknown;
        };

        if components.is_empty() {
            return ArtifactClass::Unknown;
        }

        let file_artifact_class = classify_file_artifact(&components);
        if file_artifact_class != ArtifactClass::Unknown {
            return file_artifact_class;
        }

        classify_named_directory(&components).unwrap_or(ArtifactClass::Unknown)
    }
}

fn normalize_components(path: &Path) -> Option<Vec<&OsStr>> {
    let mut components = Vec::new();

    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::Normal(component) => components.push(component),
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => return None,
        }
    }

    Some(components)
}

fn classify_named_directory(components: &[&OsStr]) -> Option<ArtifactClass> {
    let index = cargo_artifact_dir_index(components)?;
    let component = components.get(index)?;

    if *component == "build" {
        return (index > 0 && components.len() > index + 1).then_some(ArtifactClass::BuildScripts);
    }

    if *component == "incremental" {
        Some(ArtifactClass::Incremental)
    } else if *component == "deps" {
        Some(ArtifactClass::Deps)
    } else if *component == ".fingerprint"
        || *component == "fingerprint"
        || *component == ".rustdoc_fingerprint"
    {
        Some(ArtifactClass::Fingerprint)
    } else if *component == "doc" || *component == "docs" {
        Some(ArtifactClass::Docs)
    } else if *component == "package" {
        Some(ArtifactClass::Package)
    } else if *component == "timings" || *component == "cargo-timings" {
        Some(ArtifactClass::Timings)
    } else if *component == "tmp" || *component == "sqlx-tmp" {
        Some(ArtifactClass::Tmp)
    } else {
        None
    }
}

fn cargo_artifact_dir_index(components: &[&OsStr]) -> Option<usize> {
    if components.is_empty() {
        return None;
    }

    if is_known_artifact_dir(components[0]) {
        return Some(0);
    }

    if components.len() >= 2
        && is_profile_root(components[0])
        && is_known_artifact_dir(components[1])
    {
        return Some(1);
    }

    if components.len() >= 3
        && is_target_triple(components[0])
        && is_profile_root(components[1])
        && is_known_artifact_dir(components[2])
    {
        return Some(2);
    }

    None
}

fn classify_file_artifact(components: &[&OsStr]) -> ArtifactClass {
    let extension = file_extension(components);

    if is_known_intermediate_file_location(components) {
        if extension.is_some_and(|extension| extension == "d") {
            return ArtifactClass::DepInfo;
        }

        if extension.is_some_and(|extension| extension == "o" || extension == "obj") {
            return ArtifactClass::ObjectMetadata;
        }
    }

    if is_final_output_location(components) {
        match extension {
            Some(extension) if extension == "rlib" => return ArtifactClass::FinalRlib,
            Some(extension) if extension == "wasm" => return ArtifactClass::FinalWasm,
            Some(extension) if has_dynamic_or_static_library_extension(extension) => {
                return ArtifactClass::FinalLibrary;
            }
            Some(extension) if extension == "exe" => return ArtifactClass::FinalExecutable,
            None => return ArtifactClass::FinalExecutable,
            _ => {}
        }
    }

    if is_deps_final_output_location(components) {
        match extension {
            Some(extension) if extension == "rlib" => return ArtifactClass::FinalRlib,
            Some(extension) if extension == "wasm" => return ArtifactClass::FinalWasm,
            Some(extension) if extension == "exe" => return ArtifactClass::FinalExecutable,
            Some(extension) if has_dynamic_or_static_library_extension(extension) => {
                return ArtifactClass::FinalLibrary;
            }
            _ => {}
        }
    }

    ArtifactClass::Unknown
}

fn is_known_intermediate_file_location(components: &[&OsStr]) -> bool {
    match components {
        [profile, dir, _] => is_profile_root(profile) && *dir == "deps",
        [triple, profile, dir, _] => {
            is_target_triple(triple) && is_profile_root(profile) && *dir == "deps"
        }
        _ => false,
    }
}

fn is_final_output_location(components: &[&OsStr]) -> bool {
    match components {
        [profile, file_name] => {
            is_default_profile_root(profile) && is_plausible_profile_root_final_output(file_name)
        }
        [first, second, third] => {
            (is_target_triple(first)
                && is_default_profile_root(second)
                && is_plausible_profile_root_final_output(third))
                || (is_default_profile_root(first)
                    && *second == "examples"
                    && is_plausible_nested_final_output(third))
        }
        [triple, profile, dir, file_name] => {
            is_target_triple(triple)
                && is_default_profile_root(profile)
                && *dir == "examples"
                && is_plausible_nested_final_output(file_name)
        }
        _ => false,
    }
}

fn is_deps_final_output_location(components: &[&OsStr]) -> bool {
    match components {
        [profile, dir, _] => is_profile_root(profile) && *dir == "deps",
        [triple, profile, dir, _] => {
            is_target_triple(triple) && is_profile_root(profile) && *dir == "deps"
        }
        _ => false,
    }
}

fn is_plausible_profile_root_final_output(file_name: &OsStr) -> bool {
    is_plausible_nested_final_output(file_name) && !is_known_profile_support_entry(file_name)
}

fn is_plausible_nested_final_output(file_name: &OsStr) -> bool {
    !is_known_final_output_support_entry(file_name)
}

fn is_known_profile_support_entry(component: &OsStr) -> bool {
    is_known_final_output_support_entry(component) || component == "examples"
}

fn is_known_final_output_support_entry(component: &OsStr) -> bool {
    is_known_artifact_dir(component) || component == ".cargo-lock"
}

fn is_known_artifact_dir(component: &OsStr) -> bool {
    component == "incremental"
        || component == "deps"
        || component == "build"
        || component == ".fingerprint"
        || component == "fingerprint"
        || component == ".rustdoc_fingerprint"
        || component == "doc"
        || component == "docs"
        || component == "package"
        || component == "timings"
        || component == "cargo-timings"
        || component == "tmp"
        || component == "sqlx-tmp"
}

fn is_profile_root(component: &OsStr) -> bool {
    !is_known_artifact_dir(component)
        && component != "target"
        && component
            .to_str()
            .is_some_and(|component| !component.is_empty() && !component.contains('.'))
}

fn is_default_profile_root(component: &OsStr) -> bool {
    component == "debug" || component == "release"
}

fn is_target_triple(component: &OsStr) -> bool {
    component
        .to_str()
        .is_some_and(|component| component.matches('-').count() >= 2)
}

fn has_dynamic_or_static_library_extension(extension: &OsStr) -> bool {
    extension == "a"
        || extension == "so"
        || extension == "dylib"
        || extension == "dll"
        || extension == "lib"
}

fn file_extension<'a>(components: &'a [&'a OsStr]) -> Option<&'a OsStr> {
    file_name(components).and_then(|file_name| Path::new(file_name).extension())
}

fn file_name<'a>(components: &'a [&'a OsStr]) -> Option<&'a OsStr> {
    components.last().copied()
}
