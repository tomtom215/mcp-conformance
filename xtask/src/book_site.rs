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

/// The repository the book links into, as its pages spell it.
const REPOSITORY: &str = "github.com/tomtom215/mcp-conformance";

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
        pin_pages(&out.join(&release.tag), &release.tag)
            .map_err(|e| format!("cannot pin {}'s repository links: {e}", release.tag))?;
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

/// Points every HTML page under `dir` at `tag`'s files rather than `main`'s.
fn pin_pages(dir: &Path, tag: &str) -> io::Result<()> {
    for entry in fs::read_dir(dir)? {
        let path = entry?.path();
        if path.is_dir() {
            pin_pages(&path, tag)?;
        } else if path.extension().is_some_and(|ext| ext == "html") {
            let html = fs::read_to_string(&path)?;
            let pinned = pin_links(&html, tag);
            if pinned != html {
                fs::write(&path, pinned)?;
            }
        }
    }
    Ok(())
}

/// `html` with its links to files on the repository's `main` branch pointed at
/// `tag` instead. The pages say `main` because they are written on `main`; a
/// release's book that sent its reader to today's `main` would describe files
/// that may have changed since the release it documents.
fn pin_links(html: &str, tag: &str) -> String {
    let mut pinned = html.to_owned();
    for kind in ["tree", "blob"] {
        pinned = pinned.replace(
            &format!("{REPOSITORY}/{kind}/main/"),
            &format!("{REPOSITORY}/{kind}/{tag}/"),
        );
    }
    pinned
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
                "cannot run mdbook (install it: cargo install mdbook --locked --version 0.5.4): {e}"
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
mod tests;
