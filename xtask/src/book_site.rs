// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! The `book-site` task: build the versioned documentation site that
//! `pages.yml` deploys.
//!
//! A user installs a *release*, so the book they land on must describe that
//! release, not whatever `main` carries today. Until this task existed the site
//! was `main`'s book alone, and it told 0.6.0 users that a tool they already had
//! "ships with the next release". The site is now:
//!
//! | Path | Content |
//! |------|---------|
//! | `/` | the newest release's book — existing links keep working |
//! | `/vX.Y.Z/` | every release's book, as it was tagged |
//! | `/dev/` | `main`'s book: what the next release will say |
//! | `/versions.json` | the list the version switcher reads |
//! | `/versions.js` | the switcher and the "not the latest release" banner |
//!
//! Every version is rebuilt from git on every deploy — release tags matching
//! `vX.Y.Z` whose tree has `book/book.toml`, plus the checkout itself as `dev`
//! — so the site is a function of the repository and nothing is carried over
//! from an earlier deploy. Pre-release tags (`v0.7.0-rc.1`) get no book: the
//! switcher offers what a user can install from crates.io, and `dev` already
//! covers what is in flight. `/vX.Y.Z/` and `/dev/` pages carry `noindex`, so
//! a search engine sends readers to the current release at `/`.
//!
//! Each page loads the switcher by a path relative to the site root it was
//! built into, so the site works under any prefix (a fork's
//! `<user>.github.io/<repo>/` as well as the custom domain).

// `unreachable_pub` (rustc) and `redundant_pub_crate` (clippy nursery) make
// opposite demands about items in a binary crate's private modules; this follows
// the rustc lint and quiets the clippy one, per its own known-problems note.
#![allow(clippy::redundant_pub_crate)]

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// Where the site is written when `--out` is not given.
const DEFAULT_OUT: &str = "target/book-site";

/// The id of the book built from the checkout (`main`, on a deploy).
const DEV: &str = "dev";

/// A release tag with a book: `vMAJOR.MINOR.PATCH`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct Release {
    version: (u64, u64, u64),
    tag: String,
}

/// `cargo xtask book-site [--out DIR]`; `true` when the site was written.
pub(crate) fn run(args: &[String]) -> bool {
    let out = match args {
        [] => PathBuf::from(DEFAULT_OUT),
        [flag, dir] if flag == "--out" => PathBuf::from(dir),
        _ => {
            eprintln!("xtask: book-site — usage: cargo xtask book-site [--out DIR]");
            return false;
        }
    };
    let root = crate::workspace_root();
    let out = if out.is_absolute() {
        out
    } else {
        root.join(out)
    };
    match build(&root, &out) {
        Ok(releases) => {
            let latest = releases.first().map_or(DEV, |release| release.tag.as_str());
            eprintln!(
                "xtask: book-site — wrote {} ({} release book(s) + {DEV}; `/` is {latest})",
                out.display(),
                releases.len(),
            );
            true
        }
        Err(problem) => {
            eprintln!("xtask: book-site — {problem}");
            false
        }
    }
}

/// Builds the whole site into `out`, replacing anything already there, and
/// returns the releases it built, newest first.
fn build(root: &Path, out: &Path) -> Result<Vec<Release>, String> {
    let tags = git(root, &["tag", "--list", "v*"])?;
    let releases = released_books(&tags, |tag| {
        git(root, &["cat-file", "-e", &format!("{tag}:book/book.toml")]).is_ok()
    });
    if out.exists() {
        fs::remove_dir_all(out).map_err(|e| format!("cannot clear {}: {e}", out.display()))?;
    }
    let sources = out.join(".sources");
    for release in &releases {
        let tree = sources.join(&release.tag);
        export_tree(root, &release.tag, &tree)?;
        mdbook(&tree.join("book"), &out.join(&release.tag))?;
    }
    mdbook(&root.join("book"), &out.join(DEV))?;
    fs::remove_dir_all(&sources)
        .or_else(ignore_missing)
        .map_err(|e| e.to_string())?;

    let front = releases.first().map_or(DEV, |release| release.tag.as_str());
    copy_dir(&out.join(front), out).map_err(|e| format!("cannot copy {front} to the root: {e}"))?;

    let switcher = root.join("book/site/versions.js");
    fs::copy(&switcher, out.join("versions.js"))
        .map_err(|e| format!("cannot copy {}: {e}", switcher.display()))?;
    fs::write(out.join("versions.json"), versions_json(&releases))
        .map_err(|e| format!("cannot write versions.json: {e}"))?;

    let mut ids: Vec<&str> = releases
        .iter()
        .map(|release| release.tag.as_str())
        .collect();
    ids.push(DEV);
    annotate_pages(out, out, &ids).map_err(|e| format!("cannot annotate pages: {e}"))?;
    Ok(releases)
}

/// The release tags in `tags` (one per line) that `has_book` accepts, newest
/// first. Anything but an exact `vMAJOR.MINOR.PATCH` is skipped.
fn released_books(tags: &str, has_book: impl Fn(&str) -> bool) -> Vec<Release> {
    let mut releases: Vec<Release> = tags
        .lines()
        .map(str::trim)
        .filter_map(|tag| {
            let version = parse_release_tag(tag)?;
            has_book(tag).then(|| Release {
                version,
                tag: tag.to_owned(),
            })
        })
        .collect();
    releases.sort_unstable_by(|a, b| b.cmp(a));
    releases
}

/// `v1.2.3` → `(1, 2, 3)`; `None` for a pre-release, build metadata, a leading
/// zero, or anything else that is not a plain release tag.
fn parse_release_tag(tag: &str) -> Option<(u64, u64, u64)> {
    let mut parts = tag.strip_prefix('v')?.split('.');
    let mut next = || -> Option<u64> {
        let part = parts.next()?;
        let canonical = !part.is_empty()
            && part.bytes().all(|b| b.is_ascii_digit())
            && (part == "0" || !part.starts_with('0'));
        canonical.then(|| part.parse().ok()).flatten()
    };
    let version = (next()?, next()?, next()?);
    parts.next().is_none().then_some(version)
}

/// The document `versions.js` reads: the newest release, and every version in
/// the order the switcher lists them. `path` is relative to the site root; the
/// newest release is offered at the root itself, its canonical home.
fn versions_json(releases: &[Release]) -> String {
    let mut entries = Vec::with_capacity(releases.len() + 1);
    for (index, release) in releases.iter().enumerate() {
        let number = &release.tag[1..];
        let (label, path) = if index == 0 {
            (format!("{number} (latest)"), String::new())
        } else {
            (number.to_owned(), format!("{}/", release.tag))
        };
        entries.push(serde_json::json!({ "id": release.tag, "label": label, "path": path }));
    }
    let dev_path = if releases.is_empty() {
        String::new()
    } else {
        format!("{DEV}/")
    };
    entries.push(serde_json::json!({ "id": DEV, "label": "main (unreleased)", "path": dev_path }));
    let latest = releases.first().map(|release| release.tag.clone());
    let document = serde_json::json!({ "latest": latest, "versions": entries });
    let mut text = serde_json::to_string_pretty(&document).unwrap_or_default();
    text.push('\n');
    text
}

/// Adds the switcher (and, outside the root, `noindex`) to every HTML page
/// under `dir`. `site` is the site root; `versions` are the top-level
/// directory names that hold a version's book.
fn annotate_pages(site: &Path, dir: &Path, versions: &[&str]) -> io::Result<()> {
    for entry in fs::read_dir(dir)? {
        let path = entry?.path();
        if path.is_dir() {
            annotate_pages(site, &path, versions)?;
        } else if path.extension().is_some_and(|ext| ext == "html") {
            let relative = path.strip_prefix(site).map_err(io::Error::other)?;
            let html = fs::read_to_string(&path)?;
            if let Some(annotated) = annotate(&html, relative, versions) {
                fs::write(&path, annotated)?;
            }
        }
    }
    Ok(())
}

/// The page at `relative` (from the site root) with the switcher's `<script>`
/// — and `noindex`, unless the page is the root copy of the newest release —
/// inserted before `</head>`. `None` when the page has no `</head>`.
fn annotate(html: &str, relative: &Path, versions: &[&str]) -> Option<String> {
    let depth = relative.components().count().saturating_sub(1);
    let in_version = relative
        .components()
        .next()
        .and_then(|first| first.as_os_str().to_str())
        .is_some_and(|first| depth > 0 && versions.contains(&first));
    let mut head = String::new();
    // mdBook marks some pages (`print.html`, `toc.html`) `noindex` itself.
    if in_version && !html.contains("<meta name=\"robots\"") {
        head.push_str("<meta name=\"robots\" content=\"noindex\">\n");
    }
    head.push_str("<script src=\"");
    head.push_str(&"../".repeat(depth));
    head.push_str("versions.js\" defer></script>\n");
    let at = html.find("</head>")?;
    Some(format!("{}{head}{}", &html[..at], &html[at..]))
}

/// Writes `tag`'s tree into `dir` (`git archive | tar -x`), so the book's
/// `{{#include}}`s of files outside `book/` resolve as they did at the tag.
fn export_tree(root: &Path, tag: &str, dir: &Path) -> Result<(), String> {
    fs::create_dir_all(dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
    let mut archive = Command::new("git")
        .current_dir(root)
        .args(["archive", "--format=tar", tag])
        .stdout(Stdio::piped())
        .spawn()
        .map_err(|e| format!("cannot run git archive: {e}"))?;
    let stdout = archive.stdout.take().ok_or("git archive has no stdout")?;
    let unpacked = Command::new("tar")
        .args(["-x", "-C"])
        .arg(dir)
        .stdin(stdout)
        .status()
        .map_err(|e| format!("cannot run tar: {e}"))?;
    let archived = archive.wait().map_err(|e| format!("git archive: {e}"))?;
    if archived.success() && unpacked.success() {
        Ok(())
    } else {
        Err(format!(
            "exporting {tag} failed (git archive {archived}, tar {unpacked})"
        ))
    }
}

/// `mdbook build <book> -d <dest>`.
fn mdbook(book: &Path, dest: &Path) -> Result<(), String> {
    let status = Command::new("mdbook")
        .arg("build")
        .arg(book)
        .arg("-d")
        .arg(dest)
        .status()
        .map_err(|e| {
            format!(
                "cannot run mdbook (install it: cargo install mdbook --locked --version 0.5.3): {e}"
            )
        })?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("mdbook build {} failed ({status})", book.display()))
    }
}

/// Runs git in `root`; its stdout on success.
fn git(root: &Path, args: &[&str]) -> Result<String, String> {
    let output = Command::new("git")
        .current_dir(root)
        .args(args)
        .stderr(Stdio::null())
        .output()
        .map_err(|e| format!("cannot run git: {e}"))?;
    if output.status.success() {
        String::from_utf8(output.stdout).map_err(|e| format!("git output is not UTF-8: {e}"))
    } else {
        Err(format!("git {} failed ({})", args.join(" "), output.status))
    }
}

/// Copies the files under `from` into `to`, creating directories as needed.
fn copy_dir(from: &Path, to: &Path) -> io::Result<()> {
    fs::create_dir_all(to)?;
    for entry in fs::read_dir(from)? {
        let entry = entry?;
        let target = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir(&entry.path(), &target)?;
        } else {
            fs::copy(entry.path(), target)?;
        }
    }
    Ok(())
}

/// `Ok` for a "not found" error: removing what never existed is not a failure.
fn ignore_missing(error: io::Error) -> io::Result<()> {
    if error.kind() == io::ErrorKind::NotFound {
        Ok(())
    } else {
        Err(error)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn only_plain_release_tags_are_releases() {
        assert_eq!(parse_release_tag("v0.6.0"), Some((0, 6, 0)));
        assert_eq!(parse_release_tag("v10.20.30"), Some((10, 20, 30)));
        for not_a_release in [
            "0.6.0",
            "v0.6",
            "v0.6.0.1",
            "v0.7.0-rc.1",
            "v0.6.0+build",
            "v01.2.3",
            "v1..2",
            "vx.y.z",
            "",
        ] {
            assert_eq!(parse_release_tag(not_a_release), None, "{not_a_release}");
        }
    }

    #[test]
    fn releases_are_newest_first_by_version_not_by_text() {
        let tags = "v0.10.0\nv0.9.1\nv0.2.0\nv0.7.0-rc.1\nnot-a-tag\nv0.9.0\n";
        let releases = released_books(tags, |_| true);
        let order: Vec<&str> = releases
            .iter()
            .map(|release| release.tag.as_str())
            .collect();
        assert_eq!(order, ["v0.10.0", "v0.9.1", "v0.9.0", "v0.2.0"]);
    }

    #[test]
    fn a_tag_without_a_book_is_left_out() {
        let releases = released_books("v0.1.0\nv0.3.0\n", |tag| tag != "v0.1.0");
        assert_eq!(releases.len(), 1);
        assert_eq!(releases[0].tag, "v0.3.0");
    }

    #[test]
    fn the_newest_release_is_offered_at_the_root_and_dev_last() {
        let releases = released_books("v0.5.1\nv0.6.0\n", |_| true);
        let document: serde_json::Value = serde_json::from_str(&versions_json(&releases)).unwrap();
        assert_eq!(document["latest"], "v0.6.0");
        assert_eq!(
            document["versions"],
            serde_json::json!([
                { "id": "v0.6.0", "label": "0.6.0 (latest)", "path": "" },
                { "id": "v0.5.1", "label": "0.5.1", "path": "v0.5.1/" },
                { "id": "dev", "label": "main (unreleased)", "path": "dev/" },
            ])
        );
    }

    #[test]
    fn with_no_release_dev_is_the_root() {
        let document: serde_json::Value = serde_json::from_str(&versions_json(&[])).unwrap();
        assert_eq!(document["latest"], serde_json::Value::Null);
        assert_eq!(
            document["versions"],
            serde_json::json!([{ "id": "dev", "label": "main (unreleased)", "path": "" }])
        );
    }

    const PAGE: &str = "<html><head><title>t</title></head><body></body></html>";

    #[test]
    fn a_root_page_gets_the_switcher_and_stays_indexable() {
        let annotated = annotate(PAGE, Path::new("ci.html"), &["v0.6.0", "dev"]).unwrap();
        assert_eq!(
            annotated,
            "<html><head><title>t</title><script src=\"versions.js\" defer></script>\n\
             </head><body></body></html>"
        );
    }

    #[test]
    fn a_versioned_page_is_noindex_and_finds_the_switcher_at_the_site_root() {
        let annotated = annotate(PAGE, Path::new("v0.5.1/ci.html"), &["v0.5.1", "dev"]).unwrap();
        assert!(
            annotated.contains("<meta name=\"robots\" content=\"noindex\">"),
            "{annotated}"
        );
        assert!(
            annotated.contains("<script src=\"../versions.js\" defer>"),
            "{annotated}"
        );
        let nested = annotate(PAGE, Path::new("dev/a/b.html"), &["dev"]).unwrap();
        assert!(
            nested.contains("<script src=\"../../versions.js\" defer>"),
            "{nested}"
        );
        assert!(nested.contains("noindex"), "{nested}");
    }

    #[test]
    fn a_page_mdbook_already_marks_gets_no_second_robots_tag() {
        let marked = "<head><meta name=\"robots\" content=\"noindex\"></head>";
        let annotated = annotate(marked, Path::new("dev/print.html"), &["dev"]).unwrap();
        assert_eq!(
            annotated.matches("name=\"robots\"").count(),
            1,
            "{annotated}"
        );
    }

    #[test]
    fn a_root_directory_that_is_not_a_version_stays_indexable() {
        // mdBook writes no such directory today; a future one (say `assets/`)
        // must not be mistaken for an old version and hidden from search.
        let annotated = annotate(PAGE, Path::new("assets/x.html"), &["v0.6.0", "dev"]).unwrap();
        assert!(!annotated.contains("noindex"), "{annotated}");
        assert!(
            annotated.contains("<script src=\"../versions.js\" defer>"),
            "{annotated}"
        );
    }

    #[test]
    fn the_head_is_annotated_once_before_its_first_close() {
        let two = "<head></head><template><head></head></template>";
        let annotated = annotate(two, Path::new("x.html"), &[]).unwrap();
        assert_eq!(annotated.matches("versions.js").count(), 1);
        assert!(annotated.starts_with("<head><script"), "{annotated}");
        assert_eq!(annotate("<p>no head</p>", Path::new("x.html"), &[]), None);
    }
}
