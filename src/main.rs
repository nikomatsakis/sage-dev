#![feature(rustc_private)]

extern crate rustc_driver;
extern crate rustc_hir;
extern crate rustc_interface;
extern crate rustc_middle;
extern crate rustc_span;

use clap::Parser;
use notify::{RecursiveMode, Watcher};
use sage_inspector::{InspectionClient, ServerOptions, bind, run_server, serve_actor};
use sage_ir::Db;
use sage_ir::symbol::ModSymbol;

use sage::driver::{SageHost, run_sage_with};
use sage::inspector::{LiveInspectionProvider, SourceWatchRoots};

#[derive(clap::Parser)]
#[command(name = "cargo")]
struct Cargo {
    #[command(subcommand)]
    cmd: CargoCmd,
}

#[derive(clap::Subcommand)]
enum CargoCmd {
    /// Fast Rust analysis tool
    Sage {
        /// Select the workspace library target (required when more than one exists).
        #[arg(short, long = "package", value_name = "CRATE", global = true)]
        p: Option<String>,

        /// Expand a specific module and print the result.
        #[arg(long, value_name = "PATH")]
        module: Option<String>,

        #[command(subcommand)]
        command: Option<SageCommand>,
    },
}

#[derive(clap::Subcommand)]
enum SageCommand {
    /// Launch the Semantic Inspector web application.
    Inspect {
        /// Loopback port. Use 0 to ask the operating system for a test port.
        #[arg(long, default_value_t = 2442)]
        port: u16,

        /// Do not open the application in the default browser.
        #[arg(long)]
        no_open: bool,
    },
}

fn main() {
    let Cargo {
        cmd: CargoCmd::Sage { p, module, command },
    } = Cargo::parse();
    let cwd = std::env::current_dir().expect("no cwd");
    let packages: Vec<String> = p.into_iter().collect();

    match command {
        Some(SageCommand::Inspect { port, no_open }) => {
            if let Err(error) = run_inspector(cwd.clone(), packages.clone(), port, no_open) {
                eprintln!("sage: {error}");
                std::process::exit(2);
            }
            return;
        }
        None => {}
    }

    run_sage_with(&cwd, &packages, |sage| {
        if let Some(module_path) = &module {
            let segments: Vec<&str> = module_path.split("::").collect();
            match resolve_module_path(sage.db, sage.root, &segments) {
                Some(target) => {
                    let items = target.expanded_module_items(sage.db);
                    println!("=== ModSymbol: {} ({} items) ===", module_path, items.len());
                    for item in items {
                        println!("  {:?}", item.data(sage.db));
                    }
                }
                None => {
                    eprintln!("sage: could not resolve module path: {module_path}");
                }
            }
        } else {
            let items = sage.root.expanded_module_items(sage.db);
            println!("=== Root module ({} items) ===", items.len());
            for item in items {
                println!("  {:?}", item.data(sage.db));
            }
        }
    });
}

// ANCHOR: semantic_inspector_command_entry
fn run_inspector(
    project_dir: std::path::PathBuf,
    packages: Vec<String>,
    port: u16,
    no_open: bool,
) -> Result<(), String> {
    let mut live_host = SageHost::try_open(&project_dir, &packages)?;
    let runtime = tokio::runtime::Runtime::new()
        .map_err(|error| format!("failed to create the inspector runtime: {error}"))?;
    runtime.block_on(async move {
        let (client, actor) = InspectionClient::channel();
        let (watch_root_sender, watch_root_receiver) = std::sync::mpsc::channel();
        let actor = move || {
            let provider = LiveInspectionProvider::new(&mut live_host)
                .with_watch_root_observer(watch_root_sender);
            serve_actor(provider, actor);
        };
        std::thread::Builder::new()
            .name("sage-database-actor".to_owned())
            .spawn(actor)
            .map_err(|error| format!("failed to start the inspector database actor: {error}"))?;

        let _watcher = spawn_file_watcher(
            watch_root_receiver,
            client.clone(),
            tokio::runtime::Handle::current(),
        )?;

        let (address, listener) = bind(ServerOptions { port }).await.map_err(|error| {
            format!(
                "failed to bind the inspector at 127.0.0.1:{port}: {error}; choose another loopback port with `--port <PORT>`"
            )
        })?;
        let url = format!("http://{address}");
        println!(
            "{}",
            serde_json::json!({ "event": "ready", "url": url, "port": address.port() })
        );
        if !no_open {
            if let Err(error) = webbrowser::open(&url) {
                eprintln!(
                    "{}",
                    serde_json::json!({
                        "event": "browser-open-failed",
                        "url": url,
                        "message": error.to_string(),
                    })
                );
            }
        }
        run_server(listener, client)
            .await
            .map_err(|error| format!("the inspector server failed: {error}"))
    })
}
// ANCHOR_END: semantic_inspector_command_entry

fn spawn_file_watcher(
    watch_roots: std::sync::mpsc::Receiver<SourceWatchRoots>,
    client: InspectionClient,
    runtime: tokio::runtime::Handle,
) -> Result<std::thread::JoinHandle<()>, String> {
    let (readiness_sender, readiness_receiver) = std::sync::mpsc::channel::<Result<(), String>>();
    let thread = std::thread::Builder::new()
        .name("sage-source-watcher".to_owned())
        .spawn(move || {
            let (sender, receiver) = std::sync::mpsc::channel();
            let watcher = notify::recommended_watcher(move |event| {
                let _ = sender.send(event);
            });
            let mut watcher = match watcher {
                Ok(watcher) => watcher,
                Err(error) => {
                    log_watcher_error("watcher-creation-failed", &error);
                    let _ = readiness_sender.send(Err(format!(
                        "failed to create the source watcher: {error}"
                    )));
                    return;
                }
            };
            let roots = match watch_roots.recv() {
                Ok(roots) => roots,
                Err(error) => {
                    eprintln!(
                        "{}",
                        serde_json::json!({
                            "event": "watch-error",
                            "code": "initial-watch-roots-missing",
                            "message": error.to_string(),
                        })
                    );
                    let _ = readiness_sender.send(Err(format!(
                        "database actor did not provide initial source watch roots: {error}"
                    )));
                    return;
                }
            };
            let mut active_watches = std::collections::BTreeMap::new();
            if let Err(error) = reconfigure_watches(
                &mut watcher,
                &mut active_watches,
                desired_watch_roots(
                    &roots.workspace_root,
                    &roots.package_root,
                    &roots.source_root,
                ),
            ) {
                log_watcher_error("initial-watch-configuration-failed", &error);
                let _ = readiness_sender.send(Err(format!(
                    "failed to configure initial source watches: {error}"
                )));
                return;
            }
            if readiness_sender.send(Ok(())).is_err() {
                return;
            }

            while let Ok(first) = receiver.recv() {
                let mut paths = std::collections::BTreeSet::new();
                let mut reload_required = collect_changed_paths(first, &mut paths);
                while let Ok(next) = receiver.recv_timeout(std::time::Duration::from_millis(75)) {
                    reload_required |= collect_changed_paths(next, &mut paths);
                }
                paths.retain(|path| !ignored_watch_path(path));
                if paths.is_empty() {
                    continue;
                }

                let configuration_changed =
                    paths.iter().any(|path| workspace_configuration_path(path));
                let updates: Vec<_> = paths
                    .iter()
                    .filter(|path| path.extension().is_some_and(|extension| extension == "rs"))
                    .filter_map(|path| {
                        match std::fs::read_to_string(&path) {
                            Ok(text) => Some(sage_inspector::FileUpdate {
                                path: path.to_string_lossy().into_owned(),
                                text,
                            }),
                            Err(error) => {
                                log_watcher_io_error("source-read-failed", &path, &error);
                                None
                            }
                        }
                    })
                    .collect();
                if !configuration_changed && !updates.is_empty() {
                    match runtime.block_on(client.apply_updates(updates)) {
                        Ok(_) => continue,
                        Err(error) if error.error.code == "unrepresented-source-file" => {
                            reload_required = true;
                        }
                        Err(error) if error.error.code == "no-input-changes" => {}
                        Err(error) => {
                            log_watch_error(error);
                            continue;
                        }
                    }
                }
                reload_required |= configuration_changed;
                if reload_required {
                    let reason = sage_inspector::Issue {
                        code: if configuration_changed {
                            "workspace-configuration-changed"
                        } else {
                            "source-file-set-changed"
                        }
                        .to_owned(),
                        message: if configuration_changed {
                            "Cargo or toolchain configuration changed; the inspection host was reconstructed"
                        } else {
                            "a represented Rust source file was created or removed; the inspection host was reconstructed"
                        }
                        .to_owned(),
                    };
                    match runtime.block_on(client.reload_workspace(reason)) {
                        Ok(_) => match watch_roots
                            .recv_timeout(std::time::Duration::from_secs(1))
                        {
                            Ok(roots) => {
                                let desired = desired_watch_roots(
                                    &roots.workspace_root,
                                    &roots.package_root,
                                    &roots.source_root,
                                );
                                let mut attempt = 0_u64;
                                loop {
                                    match reconfigure_watches(
                                        &mut watcher,
                                        &mut active_watches,
                                        desired.clone(),
                                    ) {
                                        Ok(outcome) => {
                                            log_watch_cleanup_failures(outcome);
                                            break;
                                        }
                                        Err(error) => {
                                            attempt += 1;
                                            eprintln!(
                                                "{}",
                                                serde_json::json!({
                                                    "event": "watch-degraded",
                                                    "code": "replacement-watch-installation-failed",
                                                    "attempt": attempt,
                                                    "message": error.to_string(),
                                                })
                                            );
                                            std::thread::sleep(
                                                std::time::Duration::from_millis(250),
                                            );
                                        }
                                    }
                                }
                            }
                            Err(error) => {
                                eprintln!(
                                    "{}",
                                    serde_json::json!({
                                        "event": "watch-error",
                                        "code": "watch-root-update-missing",
                                        "message": error.to_string(),
                                    })
                                );
                            }
                        },
                        Err(error) => log_watch_error(error),
                    }
                }
            }
        })
        .map_err(|error| format!("failed to start the source watcher: {error}"))?;

    match readiness_receiver.recv() {
        Ok(Ok(())) => Ok(thread),
        Ok(Err(error)) => {
            let _ = thread.join();
            Err(error)
        }
        Err(error) => {
            let _ = thread.join();
            Err(format!(
                "source watcher stopped before reporting readiness: {error}"
            ))
        }
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum WatchMode {
    Recursive,
    NonRecursive,
}

fn desired_watch_roots(
    workspace_root: &std::path::Path,
    package_root: &std::path::Path,
    source_root: &std::path::Path,
) -> std::collections::BTreeMap<std::path::PathBuf, WatchMode> {
    let mut roots = std::collections::BTreeMap::new();
    roots.insert(workspace_root.to_path_buf(), WatchMode::NonRecursive);
    roots.insert(package_root.to_path_buf(), WatchMode::NonRecursive);
    for cargo_directory in [workspace_root.join(".cargo"), package_root.join(".cargo")] {
        if cargo_directory.is_dir() {
            roots.insert(cargo_directory, WatchMode::NonRecursive);
        }
    }
    roots.insert(source_root.to_path_buf(), WatchMode::Recursive);
    roots
}

fn workspace_configuration_path(path: &std::path::Path) -> bool {
    let Some(name) = path.file_name() else {
        return false;
    };
    if matches!(
        name.to_str(),
        Some("Cargo.toml" | "Cargo.lock" | "rust-toolchain" | "rust-toolchain.toml" | ".cargo")
    ) {
        return true;
    }
    path.parent()
        .and_then(std::path::Path::file_name)
        .is_some_and(|parent| parent == ".cargo")
        && matches!(name.to_str(), Some("config" | "config.toml"))
}

#[derive(Debug, Default)]
struct WatchReconfiguration {
    cleanup_failures: Vec<(std::path::PathBuf, String)>,
}

fn reconfigure_watches(
    watcher: &mut impl Watcher,
    active: &mut std::collections::BTreeMap<std::path::PathBuf, WatchMode>,
    desired: std::collections::BTreeMap<std::path::PathBuf, WatchMode>,
) -> notify::Result<WatchReconfiguration> {
    // Install every replacement first. If an installation fails, roll back
    // only the watches installed by this attempt and leave the prior complete
    // watch set active for a retry.
    let additions: Vec<_> = desired
        .iter()
        .filter(|(path, _)| !active.contains_key(*path))
        .map(|(path, mode)| (path.clone(), *mode))
        .collect();
    let mut installed: Vec<std::path::PathBuf> = Vec::new();
    for (path, mode) in additions {
        if let Err(error) = watcher.watch(&path, mode.as_notify_mode()) {
            let rollback_failures = rollback_watch_changes(watcher, active, &installed, &[]);
            return Err(watch_error_with_rollback(error, rollback_failures));
        }
        active.insert(path.clone(), mode);
        installed.push(path);
    }

    // Replacing the mode for an existing path requires an unwatch/watch pair.
    // Retain each previous mode so a later replacement failure can restore the
    // complete pre-reload configuration.
    let replacements: Vec<_> = desired
        .iter()
        .filter_map(|(path, new_mode)| {
            let old_mode = active.get(path).copied()?;
            (old_mode != *new_mode).then(|| (path.clone(), old_mode, *new_mode))
        })
        .collect();
    let mut replaced = Vec::new();
    for (path, old_mode, new_mode) in replacements {
        if let Err(error) = watcher.unwatch(&path) {
            let rollback_failures = rollback_watch_changes(watcher, active, &installed, &replaced);
            return Err(watch_error_with_rollback(error, rollback_failures));
        }
        active.remove(&path);
        if let Err(error) = watcher.watch(&path, new_mode.as_notify_mode()) {
            let mut rollback_failures = Vec::new();
            match watcher.watch(&path, old_mode.as_notify_mode()) {
                Ok(()) => {
                    active.insert(path.clone(), old_mode);
                }
                Err(restore_error) => {
                    rollback_failures.push(format!(
                        "could not restore {} to {old_mode:?}: {restore_error}",
                        path.display()
                    ));
                }
            }
            rollback_failures.extend(rollback_watch_changes(
                watcher, active, &installed, &replaced,
            ));
            return Err(watch_error_with_rollback(error, rollback_failures));
        }
        active.insert(path.clone(), new_mode);
        replaced.push((path, old_mode, new_mode));
    }

    // Cleanup is deliberately non-destructive to correctness: a failed
    // unwatch leaves a harmless extra old watch, while all desired roots are
    // already active. The caller reports the degraded cleanup state.
    let obsolete: Vec<_> = active
        .iter()
        .filter(|(path, mode)| desired.get(*path) != Some(*mode))
        .map(|(path, _)| path.clone())
        .collect();
    let mut outcome = WatchReconfiguration::default();
    for path in obsolete {
        match watcher.unwatch(&path) {
            Ok(()) => {
                active.remove(&path);
            }
            Err(error) => {
                outcome.cleanup_failures.push((path, error.to_string()));
            }
        }
    }
    Ok(outcome)
}

impl WatchMode {
    fn as_notify_mode(self) -> RecursiveMode {
        match self {
            WatchMode::Recursive => RecursiveMode::Recursive,
            WatchMode::NonRecursive => RecursiveMode::NonRecursive,
        }
    }
}

fn rollback_watch_changes(
    watcher: &mut impl Watcher,
    active: &mut std::collections::BTreeMap<std::path::PathBuf, WatchMode>,
    installed: &[std::path::PathBuf],
    replaced: &[(std::path::PathBuf, WatchMode, WatchMode)],
) -> Vec<String> {
    let mut failures = Vec::new();
    for (path, old_mode, _new_mode) in replaced.iter().rev() {
        if let Err(error) = watcher.unwatch(path) {
            failures.push(format!(
                "could not remove replacement watch {}: {error}",
                path.display()
            ));
        } else {
            active.remove(path);
        }
        match watcher.watch(path, old_mode.as_notify_mode()) {
            Ok(()) => {
                active.insert(path.clone(), *old_mode);
            }
            Err(error) => failures.push(format!(
                "could not restore watch {} to {old_mode:?}: {error}",
                path.display()
            )),
        }
    }
    for path in installed.iter().rev() {
        match watcher.unwatch(path) {
            Ok(()) => {
                active.remove(path);
            }
            Err(error) => failures.push(format!(
                "could not roll back newly installed watch {}: {error}",
                path.display()
            )),
        }
    }
    failures
}

fn watch_error_with_rollback(
    error: notify::Error,
    rollback_failures: Vec<String>,
) -> notify::Error {
    if rollback_failures.is_empty() {
        error
    } else {
        notify::Error::generic(&format!(
            "{error}; watch rollback was incomplete: {}",
            rollback_failures.join("; ")
        ))
    }
}

fn log_watch_cleanup_failures(outcome: WatchReconfiguration) {
    for (path, message) in outcome.cleanup_failures {
        eprintln!(
            "{}",
            serde_json::json!({
                "event": "watch-degraded",
                "code": "obsolete-watch-cleanup-failed",
                "path": path,
                "message": message,
            })
        );
    }
}

fn ignored_watch_path(path: &std::path::Path) -> bool {
    path.components().any(|component| {
        let component = component.as_os_str();
        component == "target" || component == ".git"
    })
}

fn collect_changed_paths(
    event: notify::Result<notify::Event>,
    paths: &mut std::collections::BTreeSet<std::path::PathBuf>,
) -> bool {
    let event = match event {
        Ok(event) => event,
        Err(error) => {
            eprintln!(
                "{}",
                serde_json::json!({
                    "event": "watch-error",
                    "code": "filesystem-notification-failed",
                    "message": error.to_string(),
                })
            );
            return false;
        }
    };
    if !event.kind.is_modify() && !event.kind.is_create() && !event.kind.is_remove() {
        return false;
    }
    let renames_source_membership = matches!(
        event.kind,
        notify::EventKind::Modify(notify::event::ModifyKind::Name(_))
    );
    let reload_required =
        (event.kind.is_create() || event.kind.is_remove() || renames_source_membership)
            && event
                .paths
                .iter()
                .any(|path| path.extension().is_some_and(|extension| extension == "rs"));
    paths.extend(event.paths);
    reload_required
}

#[cfg(test)]
mod watcher_tests {
    use super::*;

    #[derive(Default)]
    struct FakeWatcher {
        watched: std::collections::BTreeMap<std::path::PathBuf, RecursiveMode>,
        fail_watch: usize,
        fail_unwatch: usize,
        watch_calls: usize,
        fail_watch_at: Option<usize>,
    }

    impl Watcher for FakeWatcher {
        fn new<F: notify::EventHandler>(
            _event_handler: F,
            _config: notify::Config,
        ) -> notify::Result<Self> {
            Ok(Self::default())
        }

        fn watch(
            &mut self,
            path: &std::path::Path,
            recursive_mode: RecursiveMode,
        ) -> notify::Result<()> {
            self.watch_calls += 1;
            if self.fail_watch > 0 {
                self.fail_watch -= 1;
                return Err(notify::Error::generic("injected watch failure"));
            }
            if self.fail_watch_at == Some(self.watch_calls) {
                return Err(notify::Error::generic("injected watch failure"));
            }
            self.watched.insert(path.to_path_buf(), recursive_mode);
            Ok(())
        }

        fn unwatch(&mut self, path: &std::path::Path) -> notify::Result<()> {
            if self.fail_unwatch > 0 {
                self.fail_unwatch -= 1;
                return Err(notify::Error::generic("injected unwatch failure"));
            }
            self.watched.remove(path);
            Ok(())
        }

        fn kind() -> notify::WatcherKind {
            notify::WatcherKind::NullWatcher
        }
    }

    #[test]
    fn rust_source_rename_requires_workspace_reload() {
        let event = notify::Event::new(notify::EventKind::Modify(notify::event::ModifyKind::Name(
            notify::event::RenameMode::Both,
        )))
        .add_path("src/old.rs".into())
        .add_path("src/new.rs".into());
        let mut paths = std::collections::BTreeSet::new();
        assert!(collect_changed_paths(Ok(event), &mut paths));
        assert!(paths.contains(std::path::Path::new("src/old.rs")));
        assert!(paths.contains(std::path::Path::new("src/new.rs")));
    }

    #[test]
    fn generated_and_repository_metadata_paths_are_ignored() {
        assert!(ignored_watch_path(std::path::Path::new(
            "/workspace/target/debug/build/generated.rs"
        )));
        assert!(ignored_watch_path(std::path::Path::new(
            "/workspace/.git/index"
        )));
        assert!(!ignored_watch_path(std::path::Path::new(
            "/workspace/src/lib.rs"
        )));
    }

    #[test]
    fn cargo_and_toolchain_configuration_paths_require_reconstruction() {
        for path in [
            "/workspace/Cargo.toml",
            "/workspace/Cargo.lock",
            "/workspace/rust-toolchain",
            "/workspace/rust-toolchain.toml",
            "/workspace/.cargo",
            "/workspace/.cargo/config",
            "/workspace/.cargo/config.toml",
        ] {
            assert!(
                workspace_configuration_path(std::path::Path::new(path)),
                "{path}"
            );
        }
        assert!(!workspace_configuration_path(std::path::Path::new(
            "/workspace/src/config.toml"
        )));
    }

    #[test]
    fn initial_watch_configuration_failures_are_not_treated_as_ready() {
        let mut watcher = FakeWatcher {
            fail_watch: 1,
            ..FakeWatcher::default()
        };
        let mut active = std::collections::BTreeMap::new();
        let result = reconfigure_watches(
            &mut watcher,
            &mut active,
            desired_watch_roots(
                std::path::Path::new("/workspace"),
                std::path::Path::new("/workspace/member"),
                std::path::Path::new("/workspace/member/src"),
            ),
        );

        assert!(result.is_err());
        assert!(active.is_empty());
    }

    #[test]
    fn reload_watch_installation_failure_preserves_the_prior_watch_set() {
        let workspace = std::path::Path::new("/workspace");
        let package = std::path::Path::new("/workspace/member");
        let old_source = std::path::Path::new("/workspace/member/src");
        let new_source = std::path::Path::new("/workspace/member/generated/source");
        let mut watcher = FakeWatcher::default();
        let mut active = std::collections::BTreeMap::new();
        reconfigure_watches(
            &mut watcher,
            &mut active,
            desired_watch_roots(workspace, package, old_source),
        )
        .unwrap();

        watcher.fail_watch = 1;
        let result = reconfigure_watches(
            &mut watcher,
            &mut active,
            desired_watch_roots(workspace, package, new_source),
        );

        assert!(result.is_err());
        assert_eq!(
            active.get(old_source),
            Some(&WatchMode::Recursive),
            "the old complete watch set must survive until a retry succeeds"
        );
        assert!(!active.contains_key(new_source));
    }

    #[test]
    fn reload_unwatch_failure_keeps_both_old_and_replacement_roots() {
        let workspace = std::path::Path::new("/workspace");
        let package = std::path::Path::new("/workspace/member");
        let old_source = std::path::Path::new("/workspace/member/src");
        let new_source = std::path::Path::new("/workspace/member/generated/source");
        let mut watcher = FakeWatcher::default();
        let mut active = std::collections::BTreeMap::new();
        reconfigure_watches(
            &mut watcher,
            &mut active,
            desired_watch_roots(workspace, package, old_source),
        )
        .unwrap();

        watcher.fail_unwatch = 1;
        let outcome = reconfigure_watches(
            &mut watcher,
            &mut active,
            desired_watch_roots(workspace, package, new_source),
        )
        .unwrap();

        assert_eq!(outcome.cleanup_failures.len(), 1);
        assert_eq!(active.get(new_source), Some(&WatchMode::Recursive));
        assert_eq!(active.get(old_source), Some(&WatchMode::Recursive));
    }

    #[test]
    fn failed_same_path_mode_replacement_restores_the_previous_recursive_watch() {
        let workspace = std::path::Path::new("/workspace");
        let package = std::path::Path::new("/workspace/member");
        let nested_source = std::path::Path::new("/workspace/member/generated/source");
        let mut watcher = FakeWatcher::default();
        let mut active = std::collections::BTreeMap::new();

        // A root-level `lib.rs` makes the package root itself the recursively
        // watched source directory.
        reconfigure_watches(
            &mut watcher,
            &mut active,
            desired_watch_roots(workspace, package, package),
        )
        .unwrap();
        assert_eq!(active.get(package), Some(&WatchMode::Recursive));

        // Moving the source below the package first installs the genuinely new
        // nested root, then replaces the package-root mode. Fail that mode
        // replacement and require the entire old configuration to be restored.
        watcher.fail_watch_at = Some(watcher.watch_calls + 2);
        let result = reconfigure_watches(
            &mut watcher,
            &mut active,
            desired_watch_roots(workspace, package, nested_source),
        );

        assert!(result.is_err());
        assert_eq!(active.get(package), Some(&WatchMode::Recursive));
        assert_eq!(
            watcher.watched.get(package),
            Some(&RecursiveMode::Recursive)
        );
        assert!(!active.contains_key(nested_source));
        assert!(!watcher.watched.contains_key(nested_source));
    }

    #[test]
    fn watch_roots_follow_a_reloaded_target_source_tree() {
        let workspace = std::path::Path::new("/workspace");
        let package = std::path::Path::new("/workspace/member");
        let initial = desired_watch_roots(
            workspace,
            package,
            std::path::Path::new("/workspace/member/src"),
        );
        let reloaded = desired_watch_roots(
            workspace,
            package,
            std::path::Path::new("/workspace/member/generated/source"),
        );

        assert_eq!(initial.get(workspace), Some(&WatchMode::NonRecursive));
        assert_eq!(initial.get(package), Some(&WatchMode::NonRecursive));
        assert_eq!(
            initial.get(std::path::Path::new("/workspace/member/src")),
            Some(&WatchMode::Recursive)
        );
        assert!(!reloaded.contains_key(std::path::Path::new("/workspace/member/src")));
        assert_eq!(
            reloaded.get(std::path::Path::new("/workspace/member/generated/source")),
            Some(&WatchMode::Recursive)
        );
    }

    #[test]
    fn reconfiguration_removes_the_old_source_tree_and_watches_the_new_one() {
        let workspace = std::path::Path::new("/workspace");
        let package = std::path::Path::new("/workspace/member");
        let old_source = std::path::Path::new("/workspace/member/src");
        let new_source = std::path::Path::new("/workspace/member/generated/source");
        let mut watcher = FakeWatcher::default();
        let mut active = std::collections::BTreeMap::new();

        reconfigure_watches(
            &mut watcher,
            &mut active,
            desired_watch_roots(workspace, package, old_source),
        )
        .unwrap();
        reconfigure_watches(
            &mut watcher,
            &mut active,
            desired_watch_roots(workspace, package, new_source),
        )
        .unwrap();

        assert!(!watcher.watched.contains_key(old_source));
        assert_eq!(
            watcher.watched.get(new_source),
            Some(&RecursiveMode::Recursive)
        );
        assert_eq!(watcher.watched.len(), active.len());
    }
}

fn log_watch_error(error: sage_inspector::ErrorResponse) {
    eprintln!(
        "{}",
        serde_json::json!({
            "event": "watch-error",
            "code": error.error.code,
            "message": error.error.message,
        })
    );
}

fn log_watcher_io_error(code: &str, path: &std::path::Path, error: &std::io::Error) {
    eprintln!(
        "{}",
        serde_json::json!({
            "event": "watch-error",
            "code": code,
            "path": path,
            "message": error.to_string(),
        })
    );
}

fn log_watcher_error(code: &str, error: &notify::Error) {
    eprintln!(
        "{}",
        serde_json::json!({
            "event": "watch-error",
            "code": code,
            "message": error.to_string(),
        })
    );
}

fn resolve_module_path<'db>(
    db: &'db dyn Db,
    root: ModSymbol<'db>,
    segments: &[&str],
) -> Option<ModSymbol<'db>> {
    let mut current = root;
    for &seg in segments {
        let items = current.expanded_module_items(db);
        let found = items.iter().find(|item| {
            item.name(db)
                .map(|(name, _)| name.text(db) == seg)
                .unwrap_or(false)
        });
        match found {
            Some(item) => match item.module(db) {
                Some(m) => current = m,
                None => return None,
            },
            None => return None,
        }
    }
    Some(current)
}
