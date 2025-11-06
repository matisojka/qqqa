use anyhow::{Context, Result, anyhow};
use fs_err as fs;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Location of config dir relative to the home directory.
const CONFIG_DIR_NAME: &str = ".qq";
const CONFIG_FILE_NAME: &str = "config.json";

#[derive(Debug)]
pub struct InitExistsError {
    pub path: PathBuf,
}

impl std::fmt::Display for InitExistsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Config already exists at {}", self.path.display())
    }
}

impl std::error::Error for InitExistsError {}

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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_effort: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub default_profile: String,
    pub model_providers: HashMap<String, ModelProvider>,
    pub profiles: HashMap<String, Profile>,
    #[serde(default)]
    pub include_history: bool,
    /// Optional flag to control emoji usage in prompts.
    /// If present and not equal to "0" or "false" (case-insensitive), emojis are disabled.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub no_emoji: Option<String>,
    /// Optional list of additional commands allowed for qa execute_command.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command_allowlist: Option<Vec<String>>,
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
        model_providers.insert(
            "anthropic".to_string(),
            ModelProvider {
                name: "Anthropic".to_string(),
                base_url: "https://api.anthropic.com/v1".to_string(),
                env_key: "ANTHROPIC_API_KEY".to_string(),
                api_key: None,
            },
        );

        let mut profiles = HashMap::new();
        profiles.insert(
            "openai".to_string(),
            Profile {
                model_provider: "openai".to_string(),
                model: "gpt-5-mini".to_string(),
                reasoning_effort: None,
            },
        );
        profiles.insert(
            "groq".to_string(),
            Profile {
                model_provider: "groq".to_string(),
                model: "openai/gpt-oss-20b".to_string(),
                reasoning_effort: None,
            },
        );
        profiles.insert(
            "anthropic".to_string(),
            Profile {
                model_provider: "anthropic".to_string(),
                model: "claude-3-5-sonnet-20241022".to_string(),
                reasoning_effort: None,
            },
        );

        Self {
            default_profile: "groq".to_string(),
            model_providers,
            profiles,
            include_history: false,
            no_emoji: None,
            command_allowlist: None,
        }
    }
}

/// The resolved configuration for a run: provider, model, base URL and api key.
#[derive(Debug, Clone)]
pub struct EffectiveProfile {
    pub provider_key: String,
    pub model: String,
    pub base_url: String,
    pub api_key: String,
    pub reasoning_effort: Option<String>,
}

impl Config {
    /// Load config from disk or create a default one on first run.
    pub fn load_or_init(debug: bool) -> Result<(Self, PathBuf)> {
        let home = dirs::home_dir().ok_or_else(|| anyhow!("Could not determine home directory"))?;
        let dir = home.join(CONFIG_DIR_NAME);
        let path = dir.join(CONFIG_FILE_NAME);

        if !dir.exists() {
            fs::create_dir_all(&dir)
                .with_context(|| format!("Creating config dir: {}", dir.display()))?;
            set_permissions_dir(&dir, debug).ok();
        }

        if path.exists() {
            let bytes =
                fs::read(&path).with_context(|| format!("Reading config: {}", path.display()))?;
            let cfg: Config =
                serde_json::from_slice(&bytes).with_context(|| "Parsing config JSON")?;
            set_permissions_file(&path, debug).ok();
            Ok((cfg, path))
        } else {
            let cfg = Config::default();
            let json = serde_json::to_vec_pretty(&cfg).unwrap();
            fs::write(&path, json)
                .with_context(|| format!("Writing default config: {}", path.display()))?;
            set_permissions_file(&path, debug).ok();
            Ok((cfg, path))
        }
    }

    /// Save config back to the given path with pretty JSON and safe permissions.
    pub fn save(&self, path: &Path, debug: bool) -> Result<()> {
        let json = serde_json::to_vec_pretty(self).with_context(|| "Serializing config JSON")?;
        fs::write(path, json).with_context(|| format!("Writing config: {}", path.display()))?;
        set_permissions_file(path, debug).ok();
        Ok(())
    }

    /// Whether the no-emoji flag is effectively enabled.
    pub fn no_emoji_enabled(&self) -> bool {
        match &self.no_emoji {
            None => false,
            Some(v) => {
                let v = v.trim().to_ascii_lowercase();
                !(v.is_empty() || v == "0" || v == "false")
            }
        }
    }

    /// Commands explicitly allowed by the user for qa's execute_command tool.
    pub fn command_allowlist(&self) -> Vec<String> {
        self.command_allowlist.clone().unwrap_or_default()
    }

    /// Whether terminal history should be included in prompts by default.
    pub fn history_enabled(&self) -> bool {
        self.include_history
    }

    /// Persist the history preference toggle.
    pub fn set_history_enabled(&mut self, enabled: bool) {
        self.include_history = enabled;
    }

    /// Add a command to the custom allowlist. Returns true if the command was newly inserted.
    pub fn add_command_to_allowlist(&mut self, command: &str) -> bool {
        let entry = self.command_allowlist.get_or_insert_with(Vec::new);
        if entry.iter().any(|c| c == command) {
            false
        } else {
            entry.push(command.to_string());
            entry.sort();
            entry.dedup();
            true
        }
    }

    /// Resolve the effective profile based on optional CLI overrides for profile name and model.
    pub fn resolve_profile(
        &self,
        profile_opt: Option<&str>,
        model_override: Option<&str>,
    ) -> Result<EffectiveProfile> {
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
        let api_key = if let Some(k) = provider.api_key.clone() {
            k
        } else {
            std::env::var(&provider.env_key).map_err(|_| {
                anyhow!(
                    "Missing API key: set '{}' env var or add 'api_key' to provider '{}' in config",
                    provider.env_key,
                    provider_key
                )
            })?
        };

        Ok(EffectiveProfile {
            provider_key: provider_key.clone(),
            model,
            base_url,
            api_key,
            reasoning_effort: profile.reasoning_effort.clone(),
        })
    }

    /// Interactive initializer that writes a fresh config and allows choosing
    /// the default profile and optionally storing an API key inline.
    /// This will overwrite an existing config file.
    pub fn init_interactive(debug: bool) -> Result<PathBuf> {
        use std::io::{self, Write};

        let home = dirs::home_dir().ok_or_else(|| anyhow!("Could not determine home directory"))?;
        let dir = home.join(CONFIG_DIR_NAME);
        let path = dir.join(CONFIG_FILE_NAME);

        if !dir.exists() {
            fs::create_dir_all(&dir)
                .with_context(|| format!("Creating config dir: {}", dir.display()))?;
            set_permissions_dir(&dir, debug).ok();
        }

        if path.exists() {
            return Err(InitExistsError { path: path.clone() }.into());
        }

        let mut cfg = Config::default();

        println!("qqqa init — set up your provider and API key");
        println!("\nChoose default profile:");
        println!("  [1] Groq  — openai/gpt-oss-20b (fast, cheap)");
        println!("  [2] OpenAI — gpt-5-mini (slower, a bit smarter)");
        println!("  [3] Anthropic — claude-3-5-sonnet-20241022 (Claude by Anthropic)");
        print!("Enter 1, 2, or 3 [1]: ");
        io::stdout().flush().ok();
        let mut choice = String::new();
        io::stdin().read_line(&mut choice).ok();
        let choice = choice.trim();
        match choice {
            "2" | "openai" => cfg.default_profile = "openai".to_string(),
            "3" | "anthropic" => cfg.default_profile = "anthropic".to_string(),
            _ => cfg.default_profile = "groq".to_string(),
        }

        // Ask for API key for the chosen provider (optional).
        let provider_key = cfg.default_profile.clone();
        let provider = cfg
            .model_providers
            .get(&provider_key)
            .ok_or_else(|| anyhow!("Internal error: missing provider {}", provider_key))?
            .clone();

        let env_hint = provider.env_key.clone();
        println!(
            "\nEnter {} (optional). Leave empty to use env var {}.",
            provider.name, env_hint
        );
        print!("{}: ", provider.name);
        io::stdout().flush().ok();
        let mut key_in = String::new();
        io::stdin().read_line(&mut key_in).ok();
        let key_in = key_in.trim().to_string();

        if !key_in.is_empty() {
            if let Some(mp) = cfg.model_providers.get_mut(&provider_key) {
                mp.api_key = Some(key_in);
            }
        } else {
            // No inline key; check if env is present and warn if missing.
            if std::env::var(&env_hint).is_err() {
                println!(
                    "Hint: export {}=YOUR_KEY (e.g., add to your shell profile).",
                    env_hint
                );
            }
        }

        println!("\nShare recent `qq` / `qa` commands with the model?");
        println!("  Pros: gives follow-up questions more context without copy/paste.");
        println!(
            "  Cons: sends the last 10 `qq`/`qa` commands from your shell history to the LLM."
        );
        println!("You can change this later per run with CLI flags.");
        print!("Upload those commands by default? [y/N]: ");
        io::stdout().flush().ok();
        let mut history_choice = String::new();
        io::stdin().read_line(&mut history_choice).ok();
        let history_choice = history_choice.trim().to_ascii_lowercase();
        let history_enabled = matches!(history_choice.as_str(), "y" | "yes");
        cfg.set_history_enabled(history_enabled);

        let json = serde_json::to_vec_pretty(&cfg)?;
        fs::write(&path, json).with_context(|| format!("Writing config: {}", path.display()))?;
        set_permissions_file(&path, debug).ok();
        println!(
            "\nWrote {} with default profile '{}'.",
            path.display(),
            cfg.default_profile
        );
        Ok(path)
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
