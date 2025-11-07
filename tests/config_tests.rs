use qqqa::config::Config;
use serial_test::serial;

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
