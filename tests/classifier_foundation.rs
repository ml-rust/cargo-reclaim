use cargo_reclaim::{ArtifactClass, PolicyKind, classify_target_relative_path};

#[test]
fn stable_directories_classify_from_target_relative_paths() {
    for (path, artifact_class) in [
        ("debug/incremental", ArtifactClass::Incremental),
        ("debug/deps", ArtifactClass::Deps),
        ("debug/build/example-123", ArtifactClass::BuildScripts),
        ("debug/.fingerprint/example-123", ArtifactClass::Fingerprint),
        ("doc/example/index.html", ArtifactClass::Docs),
        ("package/example.crate", ArtifactClass::Package),
        ("timings/cargo-timing.html", ArtifactClass::Timings),
        ("tmp/work", ArtifactClass::Tmp),
    ] {
        assert_eq!(classify_target_relative_path(path), artifact_class);
    }
}

#[test]
fn intermediate_files_classify_only_in_known_locations() {
    for (path, artifact_class) in [
        ("debug/deps/example.d", ArtifactClass::DepInfo),
        ("release/deps/example.o", ArtifactClass::ObjectMetadata),
        (
            "x86_64-unknown-linux-gnu/debug/deps/example.obj",
            ArtifactClass::ObjectMetadata,
        ),
    ] {
        assert_eq!(classify_target_relative_path(path), artifact_class);
    }

    assert_eq!(
        classify_target_relative_path("deps/example.d"),
        ArtifactClass::Deps
    );
    assert_eq!(
        classify_target_relative_path("elsewhere/example.o"),
        ArtifactClass::Unknown
    );
}

#[test]
fn protected_output_directories_remain_policy_protected() {
    for (path, artifact_class) in [
        ("doc/example/index.html", ArtifactClass::Docs),
        ("package/example.crate", ArtifactClass::Package),
        ("cargo-timings/cargo-timing.html", ArtifactClass::Timings),
    ] {
        assert_eq!(classify_target_relative_path(path), artifact_class);
        assert!(PolicyKind::is_default_protected_output(artifact_class));
    }
}

#[test]
fn final_outputs_classify_from_profile_roots() {
    for (path, artifact_class) in [
        ("debug/example", ArtifactClass::FinalExecutable),
        ("release/example.exe", ArtifactClass::FinalExecutable),
        ("debug/libexample.so", ArtifactClass::FinalLibrary),
        ("release/libexample.a", ArtifactClass::FinalLibrary),
        ("debug/libexample.rlib", ArtifactClass::FinalRlib),
        ("release/example.wasm", ArtifactClass::FinalWasm),
        (
            "x86_64-unknown-linux-gnu/debug/example",
            ArtifactClass::FinalExecutable,
        ),
    ] {
        assert_eq!(classify_target_relative_path(path), artifact_class);
    }
}

#[test]
fn basename_only_final_outputs_are_unknown() {
    for path in [
        "example",
        "example.exe",
        "libexample.so",
        "libexample.rlib",
        "example.wasm",
    ] {
        assert_eq!(classify_target_relative_path(path), ArtifactClass::Unknown);
    }
}

#[test]
fn unrelated_nested_target_like_paths_are_unknown() {
    for path in [
        "dist/example",
        "dist/libexample.dylib",
        "dist/libexample.rlib",
        "dist/example.wasm",
        "workspace/target/debug/example",
        "workspace/target/debug/deps/example.d",
        "target/debug/libexample.rlib",
        "elsewhere/debug/example",
    ] {
        assert_eq!(classify_target_relative_path(path), ArtifactClass::Unknown);
    }
}

#[test]
fn empty_dot_and_unclear_paths_are_unknown() {
    for path in [
        "",
        ".",
        "./.",
        "../debug/example",
        "debug/../example",
        "debug/build",
    ] {
        assert_eq!(classify_target_relative_path(path), ArtifactClass::Unknown);
    }
}

#[test]
fn dot_segments_are_lexically_ignored() {
    assert_eq!(
        classify_target_relative_path("./debug/./incremental"),
        ArtifactClass::Incremental
    );
}
