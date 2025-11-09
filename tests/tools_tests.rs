use qqqa::perms::{clear_custom_allowlist, ensure_safe_path};
use qqqa::shell::ShellKind;
use qqqa::tools::parse_tool_call;
use qqqa::tools::read_file;
use qqqa::tools::write_file;
use serial_test::serial;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, OnceLock};

struct TempCwdGuard {
    previous: PathBuf,
    _lock: MutexGuard<'static, ()>,
}

impl TempCwdGuard {
    fn new(dir: &Path) -> Self {
        static CWD_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        let lock = CWD_LOCK.get_or_init(|| Mutex::new(())).lock().unwrap();
        let previous = std::env::current_dir().expect("read current dir");
        std::env::set_current_dir(dir).expect("set current dir");
        Self {
            previous,
            _lock: lock,
        }
    }
}

impl Drop for TempCwdGuard {
    fn drop(&mut self) {
        let _ = std::env::set_current_dir(&self.previous);
    }
}

#[test]
#[serial]
fn read_and_write_file_tools_and_path_safety() {
    // Set HOME and CWD to a temp folder for deterministic safety checks.
    let temp = tempfile::tempdir().unwrap();
    clear_custom_allowlist();
    unsafe {
        std::env::set_var("HOME", temp.path());
    }
    let _cwd_guard = TempCwdGuard::new(temp.path());

    // write_file tool
    let out = write_file::run(write_file::Args {
        path: "dir/file.txt".into(),
        content: "hello".into(),
    })
    .unwrap();
    assert!(out.contains("Wrote"));
    assert_eq!(
        std::fs::read_to_string(temp.path().join("dir/file.txt")).unwrap(),
        "hello"
    );

    // read_file tool
    let content = read_file::run(read_file::Args {
        path: "dir/file.txt".into(),
    })
    .unwrap();
    assert_eq!(content, "hello");

    // unsafe path outside HOME/CWD (unix only; skip on others)
    #[cfg(unix)]
    {
        let res = ensure_safe_path(&PathBuf::from("/tmp/should_be_blocked"));
        assert!(res.is_err(), "path outside HOME/CWD should be blocked");
    }

    // traversal escape should be denied
    let traversal = write_file::run(write_file::Args {
        path: "../../outside.txt".into(),
        content: "nope".into(),
    });
    assert!(
        traversal.is_err(),
        "expected traversal attempt to be blocked"
    );

    #[cfg(unix)]
    {
        use std::os::unix::fs::symlink;
        let link_path = temp.path().join("link_to_etc");
        let _ = symlink("/etc", &link_path);
        let res = write_file::run(write_file::Args {
            path: "link_to_etc/qqqa-test.conf".into(),
            content: "nope".into(),
        });
        assert!(res.is_err(), "expected symlink escape to be blocked");
    }
}

#[test]
fn parse_tool_call_valid_and_invalid() {
    let json = r#"{"tool":"read_file","arguments":{"path":"a.txt"}}"#;
    let call = parse_tool_call(json).expect("should parse tool call");
    match call {
        qqqa::tools::ToolCall::ReadFile(args) => assert_eq!(args.path, "a.txt"),
        _ => panic!("unexpected tool type"),
    }

    let bad = r#"{"tool":"unknown","arguments":{}}"#;
    assert!(parse_tool_call(bad).is_err());
}

#[cfg(unix)]
#[tokio::test]
#[serial]
async fn execute_command_runs_and_captures_output() {
    let temp = tempfile::tempdir().unwrap();
    unsafe {
        std::env::set_var("HOME", temp.path());
    }
    let _cwd_guard = TempCwdGuard::new(temp.path());

    let res = qqqa::tools::execute_command::run(
        qqqa::tools::execute_command::Args {
            command: "echo test123".into(),
            cwd: None,
        },
        true,
        true,
        ShellKind::Posix,
        None,
    )
    .await
    .expect("execute_command should succeed");
    assert!(res.contains("stdout"));
    assert!(res.contains("test123"));
}

#[cfg(unix)]
#[tokio::test]
#[serial]
async fn execute_command_honors_pty_force_flag() {
    let temp = tempfile::tempdir().unwrap();
    unsafe {
        std::env::set_var("HOME", temp.path());
    }
    let _cwd_guard = TempCwdGuard::new(temp.path());

    let probe = "env sh -lc '[ -t 1 ] && echo tty || echo notty'";
    let baseline = qqqa::tools::execute_command::run(
        qqqa::tools::execute_command::Args {
            command: probe.into(),
            cwd: None,
        },
        true,
        false,
        ShellKind::Posix,
        None,
    )
    .await
    .expect("baseline execute_command should succeed");
    assert!(baseline.contains("notty"));

    unsafe {
        std::env::set_var("QQQA_FORCE_PTY", "1");
    }
    let forced = qqqa::tools::execute_command::run(
        qqqa::tools::execute_command::Args {
            command: probe.into(),
            cwd: None,
        },
        true,
        false,
        ShellKind::Posix,
        None,
    )
    .await
    .expect("execute_command with forced PTY should succeed");

    unsafe {
        std::env::remove_var("QQQA_FORCE_PTY");
    }

    assert!(
        forced.contains("tty"),
        "expected PTY-enabled command to see a tty, got: {}",
        forced
    );
}

#[cfg(unix)]
#[tokio::test]
#[serial]
async fn execute_command_respects_disable_flag() {
    let temp = tempfile::tempdir().unwrap();
    unsafe {
        std::env::set_var("HOME", temp.path());
    }
    let _cwd_guard = TempCwdGuard::new(temp.path());

    let probe = "env sh -lc '[ -t 1 ] && echo tty || echo notty'";
    unsafe {
        std::env::set_var("QQQA_FORCE_PTY", "1");
    }
    let forced = qqqa::tools::execute_command::run(
        qqqa::tools::execute_command::Args {
            command: probe.into(),
            cwd: None,
        },
        true,
        false,
        ShellKind::Posix,
        None,
    )
    .await
    .expect("forced PTY should succeed");

    unsafe {
        std::env::remove_var("QQQA_FORCE_PTY");
        std::env::set_var("QQQA_DISABLE_PTY", "1");
    }

    let res = qqqa::tools::execute_command::run(
        qqqa::tools::execute_command::Args {
            command: probe.into(),
            cwd: None,
        },
        true,
        false,
        ShellKind::Posix,
        None,
    )
    .await
    .expect("execute_command fallback should succeed");

    unsafe {
        std::env::remove_var("QQQA_DISABLE_PTY");
    }

    assert!(forced.contains("tty"));
    assert!(
        res.contains("notty"),
        "expected fallback execution to behave like non-PTY run, got: {}",
        res
    );
}

#[cfg(unix)]
#[tokio::test]
async fn execute_command_streams_stdout_chunks() {
    let temp = tempfile::tempdir().unwrap();
    unsafe {
        std::env::set_var("HOME", temp.path());
    }
    let _cwd_guard = TempCwdGuard::new(temp.path());

    let mut seen = Vec::new();
    let mut printer = |chunk: qqqa::tools::execute_command::StreamChunk| {
        if matches!(chunk.kind, qqqa::tools::execute_command::StreamKind::Stdout) {
            seen.push(String::from_utf8_lossy(chunk.data).to_string());
        }
    };

    let summary = qqqa::tools::execute_command::run(
        qqqa::tools::execute_command::Args {
            command: "env sh -lc 'echo first; echo second'".into(),
            cwd: None,
        },
        true,
        false,
        ShellKind::Posix,
        Some(&mut printer),
    )
    .await
    .expect("execute_command should stream output");

    assert!(summary.contains("first"));
    assert!(summary.contains("second"));
    assert!(seen.iter().any(|line| line.contains("first")));
    assert!(seen.iter().any(|line| line.contains("second")));
}
