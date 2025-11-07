use assert_cmd::cargo::cargo_bin_cmd;
use fs_err as fs;
use qqqa::config::Config;
use serial_test::serial;
use tempfile::tempdir;

fn run_init_with_bin(bin: &str, input: &str) -> Config {
    let home = tempdir().expect("temp dir");
    let home_path = home.path().to_path_buf();
    let mut cmd = match bin {
        "qq" => cargo_bin_cmd!("qq"),
        "qa" => cargo_bin_cmd!("qa"),
        other => panic!("Unsupported bin '{}'", other),
    };
    cmd.arg("--init")
        .env("HOME", &home_path)
        // Remove env vars so prompts behave consistently in tests.
        .env_remove("GROQ_API_KEY")
        .env_remove("OPENAI_API_KEY")
        .env_remove("OPENROUTER_API_KEY")
        .env_remove("ANTHROPIC_API_KEY")
        .env_remove("OLLAMA_API_KEY")
        .write_stdin(input);
    cmd.assert().success();

    let config_path = home_path.join(".qq").join("config.json");
    let bytes = fs::read(&config_path).expect("config json");
    serde_json::from_slice(&bytes).expect("parse config")
}

fn run_qq_init(input: &str) -> Config {
    run_init_with_bin("qq", input)
}

fn run_qa_init(input: &str) -> Config {
    run_init_with_bin("qa", input)
}

#[test]
#[serial]
fn local_provider_uses_placeholder_api_key_when_env_missing() {
    unsafe {
        std::env::remove_var("OLLAMA_API_KEY");
    }
    let cfg = Config::default();
    let eff = cfg
        .resolve_profile(Some("ollama"), None)
        .expect("ollama profile should resolve");
    assert_eq!(eff.api_key, "qqqa-local-placeholder");
    assert!(eff.is_local);
}

#[test]
#[serial]
fn init_defaults_to_openrouter_profile_and_keeps_history_off() {
    let cfg = run_qq_init("\n\nn\n");
    assert_eq!(cfg.default_profile, "openrouter");
    assert!(!cfg.history_enabled());
}

#[test]
#[serial]
fn init_allows_custom_local_base_url() {
    let custom_base = "http://127.0.0.1:1234/v1";
    let input = format!("5\n\n{}\nn\n", custom_base);
    let cfg = run_qq_init(&input);
    assert_eq!(cfg.default_profile, "ollama");
    let provider = cfg
        .model_providers
        .get("ollama")
        .expect("ollama provider present");
    assert_eq!(provider.base_url, custom_base);
}

#[test]
#[serial]
fn qa_init_defaults_to_openrouter_profile() {
    let cfg = run_qa_init("\n\nn\n");
    assert_eq!(cfg.default_profile, "openrouter");
    assert!(!cfg.history_enabled());
}

#[test]
#[serial]
fn qa_init_can_select_openai_without_affecting_others() {
    let cfg = run_qa_init("3\n\nn\n");
    assert_eq!(cfg.default_profile, "openai");
    let defaults = Config::default();
    assert_eq!(
        cfg.model_providers.get("groq").map(|p| &p.base_url),
        defaults.model_providers.get("groq").map(|p| &p.base_url)
    );
}

#[test]
#[serial]
fn ollama_init_does_not_mutate_remote_providers() {
    let custom_base = "http://127.0.0.1:4321/v1";
    let input = format!("5\n\n{}\nn\n", custom_base);
    let cfg = run_qa_init(&input);
    let defaults = Config::default();
    assert_eq!(
        cfg.model_providers.get("groq").map(|p| &p.base_url),
        defaults.model_providers.get("groq").map(|p| &p.base_url)
    );
    assert_eq!(
        cfg.model_providers.get("openai").map(|p| &p.base_url),
        defaults.model_providers.get("openai").map(|p| &p.base_url)
    );
    assert_eq!(
        cfg.model_providers
            .get("ollama")
            .map(|p| &p.base_url)
            .map(|s| s.as_str()),
        Some(custom_base)
    );
}
