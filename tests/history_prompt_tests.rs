use qqqa::history::read_recent_history;
use qqqa::prompt::{build_qq_prompt, build_qa_system_prompt, build_qa_user_message};

use serial_test::serial;

#[test]
#[serial]
fn history_reader_prefers_existing_files_and_limits() {
    let dir = tempfile::tempdir().unwrap();
    std::env::set_var("HOME", dir.path());

    // Create zsh history with a couple entries
    std::fs::write(
        dir.path().join(".zsh_history"),
        ": 1696971992:0;ls -la\n: 1696971993:0;echo hello\n",
    )
    .unwrap();

    let got = read_recent_history(20, true);
    assert_eq!(got, vec!["ls -la", "echo hello"]);
}

#[test]
#[serial]
fn prompt_builders_include_sections() {
    let hist = vec!["cmd1".to_string(), "cmd2".to_string()];
    let stdin_block = Some("input text\nline2");
    let qq = build_qq_prompt(Some(os_info::get().os_type()), &hist, stdin_block, "What is Rust?");
    assert!(qq.contains("Terminal History"));
    assert!(qq.contains("Input from pipe:"));
    assert!(qq.contains("Question: What is Rust?"));

    let sys = build_qa_system_prompt();
    assert!(sys.contains("Available tools"));
    let user = build_qa_user_message(Some(os_info::get().os_type()), &hist, stdin_block, "Do the thing");
    assert!(user.contains("Question: Do the thing"));
}
