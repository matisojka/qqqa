use qqqa::perms::{clear_custom_allowlist, ensure_safe_path};
use qqqa::tools::parse_tool_call;
use qqqa::tools::read_file;
use qqqa::tools::write_file;
use serial_test::serial;
use std::path::PathBuf;

#[test]
#[serial]
fn read_and_write_file_tools_and_path_safety() {
    // Set HOME and CWD to a temp folder for deterministic safety checks.
    let temp = tempfile::tempdir().unwrap();
    clear_custom_allowlist();
    unsafe {
        std::env::set_var("HOME", temp.path());
    }
    std::env::set_current_dir(temp.path()).unwrap();

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
    std::env::set_current_dir(temp.path()).unwrap();

    let res = qqqa::tools::execute_command::run(
        qqqa::tools::execute_command::Args {
            command: "echo test123".into(),
            cwd: None,
        },
        true,
        true,
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
    let script_available = std::process::Command::new("which")
        .arg("script")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);
    if !script_available {
        eprintln!("[test] skipping PTY force coverage; 'script' not available");
        return;
    }

    let temp = tempfile::tempdir().unwrap();
    unsafe {
        std::env::set_var("HOME", temp.path());
    }
    std::env::set_current_dir(temp.path()).unwrap();

    let probe = "env sh -lc '[ -t 1 ] && echo tty || echo notty'";
    let baseline = qqqa::tools::execute_command::run(
        qqqa::tools::execute_command::Args {
            command: probe.into(),
            cwd: None,
        },
        true,
        false,
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
async fn execute_command_falls_back_when_script_missing() {
    let temp = tempfile::tempdir().unwrap();
    unsafe {
        std::env::set_var("HOME", temp.path());
    }
    std::env::set_current_dir(temp.path()).unwrap();

    let original_path = std::env::var("PATH").ok();
    let fallback_dir = std::path::Path::new("/bin");
    if !fallback_dir.exists() {
        eprintln!("[test] skipping PTY fallback coverage; /bin not found");
        return;
    }
    unsafe {
        std::env::set_var("PATH", "/bin");
        std::env::set_var("QQQA_FORCE_PTY", "1");
    }

    let probe = "env sh -lc '[ -t 1 ] && echo tty || echo notty'";
    let res = qqqa::tools::execute_command::run(
        qqqa::tools::execute_command::Args {
            command: probe.into(),
            cwd: None,
        },
        true,
        false,
    )
    .await
    .expect("execute_command fallback should succeed");

    unsafe {
        if let Some(val) = original_path {
            std::env::set_var("PATH", val);
        } else {
            std::env::remove_var("PATH");
        }
        std::env::remove_var("QQQA_FORCE_PTY");
    }

    assert!(
        res.contains("notty"),
        "expected fallback execution to behave like non-PTY run, got: {}",
        res
    );
}
