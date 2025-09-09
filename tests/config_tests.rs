use qqqa::config::Config;
use serial_test::serial;
use std::fs;

#[test]
#[serial]
fn load_or_init_creates_default_config_and_permissions() {
    // Use a temp HOME to avoid touching the real user config.
    let dir = tempfile::tempdir().unwrap();
    unsafe { std::env::set_var("HOME", dir.path()); }

    let (cfg, path) = Config::load_or_init(true).expect("load_or_init should succeed");
    assert!(path.exists(), "config file should be created");
    assert_eq!(cfg.default_profile, "groq");

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let meta = fs::metadata(&path).unwrap();
        let mode = meta.permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "config file should be 600");
        let dmeta = fs::metadata(path.parent().unwrap()).unwrap();
        let dmode = dmeta.permissions().mode() & 0o777;
        assert_eq!(dmode, 0o700, "config dir should be 700");
    }
}

#[test]
#[serial]
fn resolve_profile_uses_env_api_key_and_overrides() {
    let dir = tempfile::tempdir().unwrap();
    unsafe { std::env::set_var("HOME", dir.path()); }

    // Provide env keys for both providers
    unsafe {
        std::env::set_var("GROQ_API_KEY", "test-groq");
        std::env::set_var("OPENAI_API_KEY", "test-openai");
    }

    let (cfg, _path) = Config::load_or_init(false).unwrap();

    // Default (groq)
    let eff = cfg.resolve_profile(None, None).unwrap();
    assert_eq!(eff.model, "openai/gpt-oss-20b");
    assert_eq!(eff.api_key, "test-groq");

    // Override profile and model
    let eff2 = cfg.resolve_profile(Some("openai"), Some("gpt-5-mini")).unwrap();
    assert_eq!(eff2.model, "gpt-5-mini");
    assert_eq!(eff2.api_key, "test-openai");
}
