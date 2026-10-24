// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

#![allow(clippy::unwrap_used)]

use super::*;

/// A fresh directory of this test's own.
fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "mcp-trace-capture-output-{name}-{}",
        std::process::id()
    ));
    std::fs::remove_dir_all(&dir).ok();
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// The permission bits of the file at `path`.
#[cfg(unix)]
fn mode(path: &Path) -> u32 {
    use std::os::unix::fs::PermissionsExt as _;
    std::fs::metadata(path).unwrap().permissions().mode() & 0o777
}

#[test]
fn a_suggested_path_is_one_shell_word() {
    assert_eq!(shell_quoted("traces/s-001.jsonl"), "traces/s-001.jsonl");
    assert_eq!(shell_quoted("C:/x_y,z@1%+=.jsonl"), "C:/x_y,z@1%+=.jsonl");
    assert_eq!(
        shell_quoted("dir with space/t.jsonl"),
        "'dir with space/t.jsonl'"
    );
    assert_eq!(shell_quoted("it's;$x"), r"'it'\''s;$x'");
    assert_eq!(shell_quoted("a*b"), "'a*b'");
    assert_eq!(shell_quoted(""), "''");
}

#[test]
fn a_misspelt_placeholder_is_warned_about() {
    let warning = stray_braces(Path::new("t/{sesion}.jsonl")).unwrap();
    assert!(warning.starts_with("{sesion}.jsonl has a {…} that is not {session}"));
    assert_eq!(stray_braces(Path::new("t/{x.jsonl")), None);
    assert_eq!(stray_braces(Path::new("t/x}{.jsonl")), None);
    assert_eq!(
        stray_braces(Path::new("{x}/t.jsonl")),
        None,
        "only the file name"
    );
    assert_eq!(stray_braces(Path::new("t.jsonl")), None);
}

#[test]
fn an_existing_file_is_refused_for_what_it_is_and_others_say_why() {
    let dir = scratch("existing");
    let existing = dir.join("trace.jsonl");
    std::fs::write(&existing, "keep\n").unwrap();
    let error = create(&existing, false).unwrap_err();
    assert!(error.contains("exists; appending"), "{error}");
    assert_eq!(std::fs::read_to_string(&existing).unwrap(), "keep\n");
    let missing = dir.join("absent").join("trace.jsonl");
    let error = create(&missing, false).unwrap_err();
    assert!(error.starts_with("cannot create "), "{error}");
    assert!(create(&existing, true).is_ok());
    assert_eq!(std::fs::read_to_string(&existing).unwrap(), "");
    std::fs::remove_dir_all(&dir).ok();
}

/// `--force` overwrote whatever the output path led to: a symbolic link
/// planted there had its target truncated and filled with the trace. A link is
/// now refused whether or not `--force` is given — by the check before the
/// run starts and by the create itself — and nothing is written through it.
#[cfg(unix)]
#[test]
fn a_symbolic_link_at_the_output_path_is_refused_even_with_force() {
    let dir = scratch("symlink");
    let target = dir.join("target.jsonl");
    std::fs::write(&target, "keep\n").unwrap();
    let link = dir.join("link.jsonl");
    std::os::unix::fs::symlink(&target, &link).unwrap();
    for force in [true, false] {
        let refused = check(&link, force).unwrap_err();
        assert!(refused.contains("symbolic link"), "{force}: {refused}");
    }
    let refused = create(&link, true).unwrap_err();
    assert!(refused.contains("symbolic link"), "{refused}");
    assert_eq!(
        std::fs::read_to_string(&target).unwrap(),
        "keep\n",
        "the link's target is intact"
    );
    assert!(
        link.symlink_metadata().unwrap().is_symlink(),
        "the link itself is left in place"
    );
    // A dangling link is a link too: creating through it would create its target.
    let dangling = dir.join("dangling.jsonl");
    std::os::unix::fs::symlink(dir.join("nowhere"), &dangling).unwrap();
    assert!(
        check(&dangling, true)
            .unwrap_err()
            .contains("symbolic link")
    );
    assert!(create(&dangling, true).is_err());
    assert!(
        !dir.join("nowhere").exists(),
        "nothing was created through the link"
    );
    std::fs::remove_dir_all(&dir).ok();
}

/// `--force` truncates the file that was checked, not whatever is at the path
/// by the time it is opened: a file swapped in between — the window a planted
/// link would need — is refused untouched. (`same_file` compares by device and
/// inode on Unix; elsewhere it cannot tell, and the check before the open is
/// the whole defence.)
#[test]
fn force_overwrites_the_file_it_checked_and_no_other() {
    let dir = scratch("identity");
    let checked = dir.join("checked.jsonl");
    let other = dir.join("other.jsonl");
    std::fs::write(&checked, "checked\n").unwrap();
    std::fs::write(&other, "other\n").unwrap();
    let checked_meta = checked.symlink_metadata().unwrap();
    let other_meta = other.symlink_metadata().unwrap();
    assert!(same_file(&checked_meta, &checked.metadata().unwrap()));
    assert_eq!(same_file(&checked_meta, &other_meta), !cfg!(unix));
    let swapped = overwrite(&other, &checked_meta);
    if cfg!(unix) {
        let error = swapped.unwrap_err();
        assert!(error.contains("changed while"), "{error}");
        assert_eq!(std::fs::read_to_string(&other).unwrap(), "other\n");
    }
    drop(overwrite(&checked, &checked_meta).unwrap());
    assert_eq!(std::fs::read_to_string(&checked).unwrap(), "");
    std::fs::remove_dir_all(&dir).ok();
}

/// A trace can hold credentials and secrets the session carried, so its file
/// is made readable by its owner alone — created so, and narrowed so when
/// `--force` overwrites a wider one.
#[cfg(unix)]
#[test]
fn a_trace_file_is_readable_by_its_owner_alone() {
    use std::os::unix::fs::PermissionsExt as _;
    let dir = scratch("mode");
    let fresh = dir.join("fresh.jsonl");
    drop(create(&fresh, false).unwrap());
    assert_eq!(mode(&fresh) & 0o077, 0, "{:o}", mode(&fresh));
    let wide = dir.join("wide.jsonl");
    std::fs::write(&wide, "old\n").unwrap();
    std::fs::set_permissions(&wide, std::fs::Permissions::from_mode(0o644)).unwrap();
    drop(create(&wide, true).unwrap());
    assert_eq!(std::fs::read_to_string(&wide).unwrap(), "");
    assert_eq!(mode(&wide) & 0o077, 0, "{:o}", mode(&wide));
    std::fs::remove_dir_all(&dir).ok();
}
