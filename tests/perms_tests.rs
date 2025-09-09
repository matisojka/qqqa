use qqqa::perms::ensure_safe_command;

#[test]
fn ensure_safe_command_blocks_dangerous_patterns() {
    assert!(ensure_safe_command("rm -rf /").is_err());
    assert!(ensure_safe_command("sudo apt-get update").is_err());
    assert!(ensure_safe_command("dd if=/dev/zero of=/dev/sda").is_err());
    // Benign command allowed
    assert!(ensure_safe_command("echo hello").is_ok());
}

