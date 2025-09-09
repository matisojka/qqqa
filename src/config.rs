use anyhow::{anyhow, Context, Result};
use fs_err as fs;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Location of config dir relative to the home directory.
const CONFIG_DIR_NAME: &str = ".qq";
const CONFIG_FILE_NAME: &str = "config.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelProvider {
    pub name: String,
    pub base_url: String,
    pub env_key: String,
    /// Optional inline api key in config. Env var takes precedence only if this is absent.
    #[serde(default)]
    pub api_key: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Profile {
    pub model_provider: String,
    pub model: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub default_profile: String,
    pub model_providers: HashMap<String, ModelProvider>,
    pub profiles: HashMap<String, Profile>,
}

impl Default for Config {
    fn default() -> Self {
        // Matches schema in PORTING_TO_RUST.md
        let mut model_providers = HashMap::new();
        model_providers.insert(
            "openai".to_string(),
            ModelProvider {
                name: "OpenAI".to_string(),
                base_url: "https://api.openai.com/v1".to_string(),
                env_key: "OPENAI_API_KEY".to_string(),
                api_key: None,
            },
        );
        model_providers.insert(
            "groq".to_string(),
            ModelProvider {
                name: "Groq".to_string(),
                base_url: "https://api.groq.com/openai/v1".to_string(),
                env_key: "GROQ_API_KEY".to_string(),
                api_key: None,
            },
        );

        let mut profiles = HashMap::new();
        profiles.insert(
            "openai".to_string(),
            Profile {
                model_provider: "openai".to_string(),
                model: "gpt-5".to_string(),
            },
        );
        profiles.insert(
            "openai-mini".to_string(),
            Profile {
                model_provider: "openai".to_string(),
                model: "gpt-5-mini".to_string(),
            },
        );
        profiles.insert(
            "groq".to_string(),
            Profile {
                model_provider: "groq".to_string(),
                model: "openai/gpt-oss-20b".to_string(),
            },
        );

        Self { default_profile: "groq".to_string(), model_providers, profiles }
    }
}

/// The resolved configuration for a run: provider, model, base URL and api key.
#[derive(Debug, Clone)]
pub struct EffectiveProfile {
    pub provider_key: String,
    pub model: String,
    pub base_url: String,
    pub api_key: String,
}

impl Config {
    /// Load config from disk or create a default one on first run.
    pub fn load_or_init(debug: bool) -> Result<(Self, PathBuf)> {
        let home = dirs::home_dir().ok_or_else(|| anyhow!("Could not determine home directory"))?;
        let dir = home.join(CONFIG_DIR_NAME);
        let path = dir.join(CONFIG_FILE_NAME);

        if !dir.exists() {
            fs::create_dir_all(&dir).with_context(|| format!("Creating config dir: {}", dir.display()))?;
            set_permissions_dir(&dir, debug).ok();
        }

        if path.exists() {
            let bytes = fs::read(&path).with_context(|| format!("Reading config: {}", path.display()))?;
            let cfg: Config = serde_json::from_slice(&bytes).with_context(|| "Parsing config JSON")?;
            set_permissions_file(&path, debug).ok();
            Ok((cfg, path))
        } else {
            let cfg = Config::default();
            let json = serde_json::to_vec_pretty(&cfg).unwrap();
            fs::write(&path, json).with_context(|| format!("Writing default config: {}", path.display()))?;
            set_permissions_file(&path, debug).ok();
            Ok((cfg, path))
        }
    }

    /// Resolve the effective profile based on optional CLI overrides for profile name and model.
    pub fn resolve_profile(&self, profile_opt: Option<&str>, model_override: Option<&str>) -> Result<EffectiveProfile> {
        let profile_name = profile_opt.unwrap_or(&self.default_profile);
        let profile = self
            .profiles
            .get(profile_name)
            .ok_or_else(|| anyhow!("Profile '{}' not found in config", profile_name))?;
        let provider_key = &profile.model_provider;
        let provider = self
            .model_providers
            .get(provider_key)
            .ok_or_else(|| anyhow!("Model provider '{}' not found in config", provider_key))?;

        let model = model_override.unwrap_or(&profile.model).to_string();
        let base_url = provider.base_url.clone();

        // Prefer inline api_key; else env var per env_key.
        let api_key = if let Some(k) = provider.api_key.clone() { k } else {
            std::env::var(&provider.env_key).map_err(|_| {
                anyhow!(
                    "Missing API key: set '{}' env var or add 'api_key' to provider '{}' in config",
                    provider.env_key, provider_key
                )
            })?
        };

        Ok(EffectiveProfile { provider_key: provider_key.clone(), model, base_url, api_key })
    }
}

fn set_permissions_dir(path: &Path, _debug: bool) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(path)?.permissions();
        perms.set_mode(0o700);
        fs::set_permissions(path, perms)?;
    }
    Ok(())
}

fn set_permissions_file(path: &Path, _debug: bool) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(path)?.permissions();
        perms.set_mode(0o600);
        fs::set_permissions(path, perms)?;
    }
    Ok(())
}

