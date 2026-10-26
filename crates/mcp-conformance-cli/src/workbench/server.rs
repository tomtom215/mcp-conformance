// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Starting the workbench: open the library, bind, print the launch link,
//! serve until interrupted.
//!
//! It binds `127.0.0.1` on an OS-assigned port unless told otherwise. Any
//! address that is not loopback is refused unless
//! `--dangerously-listen-non-loopback` is also given, and a workbench started
//! that way says so on stderr after its link: the library may hold
//! credentials, and the token is then the only thing between it and the
//! network.

use std::net::SocketAddr;
use std::path::Path;
use std::sync::{Arc, Mutex};

use axum::body::Body;
use axum::http::Request;

use super::api::{self, Shared};
use super::guard::Keys;
use super::state::Workbench;

/// What `serve` was asked for.
#[derive(Debug, Clone)]
pub(crate) struct Settings<'a> {
    pub(crate) library: &'a Path,
    pub(crate) listen: SocketAddr,
    pub(crate) non_loopback: bool,
    pub(crate) open: bool,
}

/// Checks the address before anything is bound.
pub(crate) fn check_address(listen: SocketAddr, non_loopback: bool) -> Result<(), String> {
    if listen.ip().is_loopback() || non_loopback {
        Ok(())
    } else {
        Err(format!(
            "{listen} is not a loopback address: the workbench shows recordings that may hold \
             credentials, so it listens on 127.0.0.1 unless --dangerously-listen-non-loopback is \
             also given"
        ))
    }
}

/// The shared state of a workbench over `library`, bound to `address`.
pub(crate) fn shared(library: &Path, address: SocketAddr) -> Result<Arc<Shared>, String> {
    let workbench = Workbench::open(library)
        .map_err(|error| format!("cannot open the library {}: {error}", library.display()))?;
    Ok(Arc::new(Shared {
        keys: Keys::new(address)?,
        workbench: Mutex::new(workbench),
    }))
}

/// The router: every request to [`api::handle`].
pub(crate) fn app(shared: Arc<Shared>) -> axum::Router {
    axum::Router::new().fallback(move |request: Request<Body>| api::handle(shared.clone(), request))
}

/// The command that opens `url` in the system's browser: a program and its
/// arguments, run without a shell.
fn browser_command(url: &str) -> (&'static str, Vec<&str>) {
    if cfg!(target_os = "macos") {
        ("open", vec![url])
    } else if cfg!(windows) {
        ("rundll32", vec!["url.dll,FileProtocolHandler", url])
    } else {
        ("xdg-open", vec![url])
    }
}

/// Starts `program` with `args` and leaves it running; why it could not be
/// started, when it could not.
fn launch(program: &str, args: &[&str]) -> Result<(), String> {
    std::process::Command::new(program)
        .args(args)
        .spawn()
        .map(drop)
        .map_err(|error| {
            format!("could not open a browser ({program}: {error}); open the link above")
        })
}

/// The one thing the session cookie cannot defend against, said in the
/// banner: browsers scope a cookie to the host, never to the port, so every
/// other server on the bound address that the browser visits is sent it. It
/// is `HttpOnly` (no page can read it), but that server receives it.
fn cookie_scope_notice(address: SocketAddr) -> String {
    format!(
        "mcp-conformance serve: browsers scope the cookie to {} as a whole, not to port {}: any \
         other server there that this browser visits is sent it",
        address.ip(),
        address.port()
    )
}

/// The warning a workbench listening on `address` prints after its link,
/// when `address` is not loopback.
fn exposure_warning(address: SocketAddr) -> Option<String> {
    (!address.ip().is_loopback()).then(|| {
        format!(
            "WARNING: listening on {address}, which is not loopback: anyone who can reach it \
             and holds the link can read the library"
        )
    })
}

/// Binds the address and opens the library: the listener and the state.
async fn start(
    settings: &Settings<'_>,
) -> Result<(tokio::net::TcpListener, SocketAddr, Arc<Shared>), String> {
    check_address(settings.listen, settings.non_loopback)?;
    let listener = tokio::net::TcpListener::bind(settings.listen)
        .await
        .map_err(|error| format!("cannot listen on {}: {error}", settings.listen))?;
    let address = listener.local_addr().map_err(|error| error.to_string())?;
    let library = settings.library.to_path_buf();
    let shared = tokio::task::spawn_blocking(move || shared(&library, address))
        .await
        .map_err(|error| error.to_string())??;
    Ok((listener, address, shared))
}

/// Runs the workbench until Ctrl-C; returns the exit code.
pub(crate) async fn run(settings: Settings<'_>) -> u8 {
    let shutdown = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    run_until(settings, shutdown, |line| println!("{line}")).await
}

/// Runs the workbench until `shutdown` completes, telling the user what it
/// serves through `say`; returns the exit code.
pub(crate) async fn run_until(
    settings: Settings<'_>,
    shutdown: impl Future<Output = ()> + Send + 'static,
    mut say: impl FnMut(String),
) -> u8 {
    let (listener, address, shared) = match start(&settings).await {
        Ok(started) => started,
        Err(why) => {
            eprintln!("mcp-conformance serve: {why}");
            return crate::EXIT_USAGE;
        }
    };
    let url = shared.keys.launch_url();
    let count = shared.lock_count();
    say(format!(
        "mcp-conformance serve: library {} ({count} recordings)",
        settings.library.display()
    ));
    say(format!("mcp-conformance serve: open {url}"));
    say("mcp-conformance serve: the link works once; Ctrl-C stops the workbench".to_owned());
    say(cookie_scope_notice(address));
    if let Some(warning) = exposure_warning(address) {
        eprintln!("mcp-conformance serve: {warning}");
    }
    if settings.open {
        let (program, args) = browser_command(&url);
        if let Err(why) = launch(program, &args) {
            eprintln!("mcp-conformance serve: {why}");
        }
    }
    match axum::serve(listener, app(shared))
        .with_graceful_shutdown(shutdown)
        .await
    {
        Ok(()) => crate::EXIT_OK,
        Err(error) => {
            eprintln!("mcp-conformance serve: {error}");
            crate::EXIT_USAGE
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;

    #[test]
    fn only_loopback_is_bound_without_the_flag() {
        for address in ["127.0.0.1:0", "[::1]:8080", "127.0.0.2:1"] {
            assert!(
                check_address(address.parse().unwrap(), false).is_ok(),
                "{address}"
            );
        }
        for address in ["0.0.0.0:0", "192.168.1.2:80", "[::]:0"] {
            let address: SocketAddr = address.parse().unwrap();
            let error = check_address(address, false).unwrap_err();
            assert!(
                error.contains("--dangerously-listen-non-loopback"),
                "{error}"
            );
            assert!(check_address(address, true).is_ok());
        }
    }

    #[test]
    fn the_cookie_notice_names_the_host_and_the_port_it_does_not_cover() {
        assert_eq!(
            cookie_scope_notice("[::1]:4200".parse().unwrap()),
            "mcp-conformance serve: browsers scope the cookie to ::1 as a whole, not to port \
             4200: any other server there that this browser visits is sent it"
        );
        assert!(cookie_scope_notice("127.0.0.1:1".parse().unwrap()).contains("127.0.0.1 as"));
    }

    #[test]
    fn only_a_non_loopback_address_is_warned_about() {
        assert_eq!(exposure_warning("127.0.0.1:4200".parse().unwrap()), None);
        assert_eq!(exposure_warning("[::1]:4200".parse().unwrap()), None);
        let warning = exposure_warning("0.0.0.0:4200".parse().unwrap()).unwrap();
        assert!(
            warning.starts_with("WARNING: listening on 0.0.0.0:4200, which is not loopback"),
            "{warning}"
        );
    }

    #[test]
    fn the_browser_is_the_platforms_opener_given_the_link_alone() {
        let (program, args) = browser_command("http://127.0.0.1:1/?t=a&b");
        let expected = if cfg!(target_os = "macos") {
            "open"
        } else if cfg!(windows) {
            "rundll32"
        } else {
            "xdg-open"
        };
        assert_eq!(program, expected);
        assert_eq!(args.last(), Some(&"http://127.0.0.1:1/?t=a&b"));
        // No shell: the link's `&` is an argument's byte, not an operator.
        assert!(args.len() <= 2);
    }

    #[test]
    fn a_launcher_that_cannot_start_says_why() {
        let missing = "mcp-conformance-no-such-browser";
        let why = launch(missing, &["http://127.0.0.1:1/"]).unwrap_err();
        assert!(
            why.contains(missing) && why.ends_with("open the link above"),
            "{why}"
        );
        #[cfg(unix)]
        assert_eq!(launch("true", &[]), Ok(()));
    }

    #[tokio::test]
    async fn it_serves_until_told_to_stop() {
        let dir = crate::workbench::fixture::library("serve");
        let (stop, stopped) = tokio::sync::oneshot::channel::<()>();
        let (lines, mut said) = tokio::sync::mpsc::unbounded_channel();
        let library = dir.to_path_buf();
        let serving = tokio::spawn(async move {
            let settings = Settings {
                library: &library,
                listen: "127.0.0.1:0".parse().unwrap(),
                non_loopback: false,
                open: false,
            };
            run_until(
                settings,
                async { stopped.await.ok().unwrap_or(()) },
                move |line| {
                    lines.send(line).ok();
                },
            )
            .await
        });
        // A banner line, or a failure naming the one that never came: the
        // server says nothing more once started, so waiting would hang.
        let mut next = async || {
            tokio::time::timeout(std::time::Duration::from_secs(10), said.recv())
                .await
                .unwrap_or_else(|elapsed| panic!("no banner line within 10 s ({elapsed})"))
                .unwrap()
        };
        let first = next().await;
        assert!(first.ends_with("(11 recordings)"), "{first}");
        let link = next().await;
        let link = link
            .strip_prefix("mcp-conformance serve: open ")
            .unwrap()
            .to_owned();
        let once = next().await;
        assert!(once.contains("the link works once"), "{once}");
        // The banner says the one thing the cookie cannot defend against:
        // browsers scope cookies to the host, not the port, so every other
        // server on 127.0.0.1 this browser visits is sent it.
        let cookie = next().await;
        assert!(
            cookie.starts_with("mcp-conformance serve: ")
                && cookie.contains("cookie")
                && cookie.contains("port")
                && cookie.contains("127.0.0.1"),
            "{cookie}"
        );
        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .unwrap();
        assert_eq!(client.get(&link).send().await.unwrap().status(), 303);
        stop.send(()).unwrap();
        assert_eq!(serving.await.unwrap(), crate::EXIT_OK);
    }

    #[tokio::test]
    async fn what_cannot_be_opened_or_bound_is_refused() {
        let dir = crate::workbench::fixture::library("serve-refused");
        // A file is not a library, and an address that is not loopback is not bound.
        let file = dir.join("everything-http.jsonl");
        for (library, listen) in [(file.as_path(), "127.0.0.1:0"), (&*dir, "0.0.0.0:0")] {
            let settings = Settings {
                library,
                listen: listen.parse().unwrap(),
                non_loopback: false,
                open: false,
            };
            assert_eq!(
                run_until(settings, async {}, |_| {}).await,
                crate::EXIT_USAGE
            );
        }
        // Nor is an address already taken.
        let taken = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let settings = Settings {
            library: &dir,
            listen: taken.local_addr().unwrap(),
            non_loopback: false,
            open: false,
        };
        assert_eq!(
            run_until(settings, async {}, |_| {}).await,
            crate::EXIT_USAGE
        );
    }
}
