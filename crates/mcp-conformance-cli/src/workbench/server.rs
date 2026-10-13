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

/// Opens `url` in the system's browser, without a shell.
fn open_browser(url: &str) {
    let (program, args): (&str, Vec<&str>) = if cfg!(target_os = "macos") {
        ("open", vec![url])
    } else if cfg!(windows) {
        ("rundll32", vec!["url.dll,FileProtocolHandler", url])
    } else {
        ("xdg-open", vec![url])
    };
    if let Err(error) = std::process::Command::new(program).args(args).spawn() {
        eprintln!(
            "mcp-conformance serve: could not open a browser ({program}: {error}); open the link above"
        );
    }
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
    if !address.ip().is_loopback() {
        eprintln!(
            "mcp-conformance serve: WARNING: listening on {address}, which is not loopback: \
             anyone who can reach it and holds the link can read the library"
        );
    }
    if settings.open {
        open_browser(&url);
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

    #[tokio::test]
    async fn it_serves_until_told_to_stop() {
        let dir = crate::workbench::fixture::library("serve");
        let (stop, stopped) = tokio::sync::oneshot::channel::<()>();
        let (lines, mut said) = tokio::sync::mpsc::unbounded_channel();
        let library = dir.clone();
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
        let first = said.recv().await.unwrap();
        assert!(first.ends_with("(11 recordings)"), "{first}");
        let link = said.recv().await.unwrap();
        let link = link
            .strip_prefix("mcp-conformance serve: open ")
            .unwrap()
            .to_owned();
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
        for (library, listen) in [
            (file.as_path(), "127.0.0.1:0"),
            (dir.as_path(), "0.0.0.0:0"),
        ] {
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
