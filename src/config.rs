use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow};
use serde::Deserialize;

use crate::agent::chat_completions::DEFAULT_CHAT_COMPLETIONS_BASE_URL;

const ENV_OPENAI_API_KEY: &str = "OPENAI_API_KEY";
const ENV_OPENAI_BASE_URL: &str = "OPENAI_BASE_URL";
const ENV_SUMMARISER_MODEL: &str = "MMR_SUMMARISER_MODEL";
const ENV_CONFIG_FILE: &str = "MMR_CONFIG_FILE";
pub const DEFAULT_SUMMARISER_MODEL: &str = "gpt-5.5";

#[derive(Debug, Clone, Default, Deserialize)]
pub struct MmrConfig {
    #[serde(default)]
    pub summarize: Option<SummarizeConfig>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SummarizeConfig {
    pub api_key: Option<String>,
    pub api_key_env: Option<String>,
    pub base_url: Option<String>,
    pub model: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedSummarizeSettings {
    pub api_key: String,
    pub base_url: String,
    pub model: String,
}

/// Resolved path to `config.json` (may not exist yet).
pub fn mmr_config_path() -> Option<PathBuf> {
    if let Ok(path) = std::env::var(ENV_CONFIG_FILE) {
        let trimmed = path.trim();
        if !trimmed.is_empty() {
            return Some(PathBuf::from(trimmed));
        }
    }

    let config_root = std::env::var("XDG_CONFIG_HOME")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(config_home_dir)?;

    Some(config_root.join("mmr").join("config.json"))
}

pub fn load_mmr_config() -> Result<Option<MmrConfig>> {
    let Some(path) = mmr_config_path() else {
        return Ok(None);
    };
    if !path.is_file() {
        return Ok(None);
    }

    let raw =
        fs::read_to_string(&path).with_context(|| format!("failed to read {}", path.display()))?;
    let parsed = serde_json::from_str::<MmrConfig>(&raw)
        .with_context(|| format!("failed to parse {}", path.display()))?;
    Ok(Some(parsed))
}

pub fn resolve_summarize_settings(cli_model: Option<&str>) -> Result<ResolvedSummarizeSettings> {
    let config = load_mmr_config()?.unwrap_or_default();
    let summarize = config.summarize.as_ref();

    let api_key = resolve_summarize_api_key(summarize)?;

    let base_url = pick_two(
        summarize.and_then(|value| value.base_url.as_deref()),
        optional_env(ENV_OPENAI_BASE_URL),
    )
    .unwrap_or_else(|| DEFAULT_CHAT_COMPLETIONS_BASE_URL.to_string());

    let model = pick_three(
        cli_model,
        summarize.and_then(|value| value.model.as_deref()),
        optional_env(ENV_SUMMARISER_MODEL),
    )
    .unwrap_or_else(|| DEFAULT_SUMMARISER_MODEL.to_string());

    Ok(ResolvedSummarizeSettings {
        api_key,
        base_url,
        model,
    })
}

pub fn summarize_api_key_configured() -> bool {
    let summarize = load_mmr_config()
        .ok()
        .flatten()
        .and_then(|config| config.summarize);
    resolve_summarize_api_key(summarize.as_ref()).is_ok()
}

fn resolve_summarize_api_key(summarize: Option<&SummarizeConfig>) -> Result<String> {
    if let Some(api_key) =
        summarize.and_then(|value| normalize_optional_str(value.api_key.as_deref()))
    {
        return Ok(api_key);
    }

    if let Some(env_name) =
        summarize.and_then(|value| normalize_optional_str(value.api_key_env.as_deref()))
    {
        return optional_env(&env_name).with_context(|| {
            format!("environment variable {env_name} (from summarize.apiKeyEnv) must be set for summarize")
        });
    }

    optional_env(ENV_OPENAI_API_KEY).ok_or_else(|| {
        let path = mmr_config_path()
            .map(|value| value.display().to_string())
            .unwrap_or_else(|| "~/.config/mmr/config.json".to_string());
        anyhow!(
            "summarize API key is not configured; set summarize.apiKey or summarize.apiKeyEnv in {path}, or set {ENV_OPENAI_API_KEY}"
        )
    })
}

pub fn summarize_endpoint_for_status() -> String {
    load_mmr_config()
        .ok()
        .and_then(|config| config)
        .and_then(|config| config.summarize)
        .and_then(|summarize| summarize.base_url)
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .or_else(|| optional_env(ENV_OPENAI_BASE_URL))
        .unwrap_or_else(|| DEFAULT_CHAT_COMPLETIONS_BASE_URL.to_string())
}

pub fn write_summarize_config_for_tests(
    home: &Path,
    base_url: &str,
    model: &str,
) -> std::io::Result<PathBuf> {
    write_summarize_config_for_tests_with_api(home, base_url, model, None, None)
}

pub fn write_summarize_config_for_tests_with_api(
    home: &Path,
    base_url: &str,
    model: &str,
    api_key: Option<&str>,
    api_key_env: Option<&str>,
) -> std::io::Result<PathBuf> {
    let path = home.join(".config").join("mmr").join("config.json");
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut summarize = serde_json::json!({
        "baseUrl": base_url,
        "model": model
    });
    if let Some(api_key) = api_key {
        summarize["apiKey"] = serde_json::Value::String(api_key.to_string());
    }
    if let Some(api_key_env) = api_key_env {
        summarize["apiKeyEnv"] = serde_json::Value::String(api_key_env.to_string());
    }
    let contents = serde_json::json!({ "summarize": summarize });
    fs::write(
        &path,
        serde_json::to_vec_pretty(&contents).expect("config json"),
    )?;
    Ok(path)
}

fn config_home_dir() -> Option<PathBuf> {
    std::env::var("HOME")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(dirs::home_dir)
        .map(|home| home.join(".config"))
}

fn pick_two(first: Option<&str>, second: Option<String>) -> Option<String> {
    normalize_optional_str(first).or(second)
}

fn pick_three(first: Option<&str>, second: Option<&str>, third: Option<String>) -> Option<String> {
    normalize_optional_str(first)
        .or_else(|| normalize_optional_str(second))
        .or(third)
}

fn normalize_optional_str(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn optional_env(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

#[cfg(test)]
mod tests {
    use std::sync::{Mutex, MutexGuard};

    use super::*;

    static ENV_TEST_LOCK: Mutex<()> = Mutex::new(());

    fn env_test_lock() -> MutexGuard<'static, ()> {
        ENV_TEST_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    #[test]
    fn mmr_config_path_uses_home_dot_config_by_default() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let path = tmp.path().join(".config").join("mmr").join("config.json");
        assert_eq!(mmr_config_path_from_home(tmp.path()), Some(path));
    }

    #[test]
    fn resolve_reads_api_key_from_config() {
        let tmp = tempfile::tempdir().expect("tempdir");
        write_summarize_config_for_tests_with_api(
            tmp.path(),
            "http://cfg/v1",
            "gpt-5.5",
            Some("config-key"),
            None,
        )
        .expect("write config");
        with_home_and_env(tmp.path(), &[], || {
            let resolved = resolve_summarize_settings(None).expect("resolve");
            assert_eq!(resolved.api_key, "config-key");
        });
    }

    #[test]
    fn resolve_reads_api_key_from_config_api_key_env() {
        let tmp = tempfile::tempdir().expect("tempdir");
        write_summarize_config_for_tests_with_api(
            tmp.path(),
            "http://cfg/v1",
            "gpt-5.5",
            None,
            Some("CUSTOM_API_KEY"),
        )
        .expect("write config");
        with_home_and_env(tmp.path(), &[("CUSTOM_API_KEY", "custom-env-key")], || {
            let resolved = resolve_summarize_settings(None).expect("resolve");
            assert_eq!(resolved.api_key, "custom-env-key");
        });
    }

    #[test]
    fn resolve_prefers_config_api_key_over_api_key_env() {
        let tmp = tempfile::tempdir().expect("tempdir");
        write_summarize_config_for_tests_with_api(
            tmp.path(),
            "http://cfg/v1",
            "gpt-5.5",
            Some("direct-key"),
            Some("CUSTOM_API_KEY"),
        )
        .expect("write config");
        with_home_and_env(tmp.path(), &[("CUSTOM_API_KEY", "custom-env-key")], || {
            let resolved = resolve_summarize_settings(None).expect("resolve");
            assert_eq!(resolved.api_key, "direct-key");
        });
    }

    #[test]
    fn resolve_reads_openai_api_key_from_env() {
        let tmp = tempfile::tempdir().expect("tempdir");
        with_home_and_env(tmp.path(), &[("OPENAI_API_KEY", "env-key")], || {
            let resolved = resolve_summarize_settings(None).expect("resolve");
            assert_eq!(resolved.api_key, "env-key");
            assert_eq!(resolved.base_url, DEFAULT_CHAT_COMPLETIONS_BASE_URL);
            assert_eq!(resolved.model, DEFAULT_SUMMARISER_MODEL);
        });
    }

    #[test]
    fn resolve_uses_config_for_base_url_and_model_when_present() {
        let tmp = tempfile::tempdir().expect("tempdir");
        write_summarize_config_for_tests(tmp.path(), "http://cfg/v1", "gpt-5.5")
            .expect("write config");
        with_home_and_env(tmp.path(), &[("OPENAI_API_KEY", "env-key")], || {
            let resolved = resolve_summarize_settings(None).expect("resolve");
            assert_eq!(resolved.api_key, "env-key");
            assert_eq!(resolved.base_url, "http://cfg/v1");
            assert_eq!(resolved.model, "gpt-5.5");
        });
    }

    #[test]
    fn resolve_uses_env_when_config_missing_fields() {
        let tmp = tempfile::tempdir().expect("tempdir");
        with_home_and_env(
            tmp.path(),
            &[
                ("OPENAI_API_KEY", "env-key"),
                ("OPENAI_BASE_URL", "http://env/v1"),
                ("MMR_SUMMARISER_MODEL", "env-model"),
            ],
            || {
                let resolved = resolve_summarize_settings(None).expect("resolve");
                assert_eq!(resolved.api_key, "env-key");
                assert_eq!(resolved.base_url, "http://env/v1");
                assert_eq!(resolved.model, "env-model");
            },
        );
    }

    #[test]
    fn resolve_prefers_api_key_env_over_openai_api_key_default() {
        let tmp = tempfile::tempdir().expect("tempdir");
        write_summarize_config_for_tests_with_api(
            tmp.path(),
            "http://cfg/v1",
            "gpt-5.5",
            None,
            Some("CUSTOM_API_KEY"),
        )
        .expect("write config");
        with_home_and_env(
            tmp.path(),
            &[
                ("CUSTOM_API_KEY", "custom-env-key"),
                ("OPENAI_API_KEY", "openai-default-key"),
            ],
            || {
                let resolved = resolve_summarize_settings(None).expect("resolve");
                assert_eq!(resolved.api_key, "custom-env-key");
            },
        );
    }

    #[test]
    fn resolve_fails_when_api_key_env_is_unset() {
        let tmp = tempfile::tempdir().expect("tempdir");
        write_summarize_config_for_tests_with_api(
            tmp.path(),
            "http://cfg/v1",
            "gpt-5.5",
            None,
            Some("MISSING_API_KEY"),
        )
        .expect("write config");
        with_home_and_env(tmp.path(), &[], || {
            let error = resolve_summarize_settings(None).expect_err("missing apiKeyEnv target");
            let message = error.to_string();
            assert!(message.contains("MISSING_API_KEY"));
            assert!(message.contains("summarize.apiKeyEnv"));
        });
    }

    #[test]
    fn resolve_fails_when_no_api_key_configured() {
        let tmp = tempfile::tempdir().expect("tempdir");
        with_home_and_env(tmp.path(), &[], || {
            let error = resolve_summarize_settings(None).expect_err("missing api key");
            let message = error.to_string();
            assert!(message.contains("summarize.apiKey"));
            assert!(message.contains("summarize.apiKeyEnv"));
            assert!(message.contains(ENV_OPENAI_API_KEY));
        });
    }

    #[test]
    fn summarize_api_key_configured_reflects_all_sources() {
        let tmp = tempfile::tempdir().expect("tempdir");
        write_summarize_config_for_tests_with_api(
            tmp.path(),
            "http://cfg/v1",
            "gpt-5.5",
            Some("inline-key"),
            None,
        )
        .expect("write config");
        with_home_and_env(tmp.path(), &[], || {
            assert!(summarize_api_key_configured());
        });

        write_summarize_config_for_tests_with_api(
            tmp.path(),
            "http://cfg/v1",
            "gpt-5.5",
            None,
            Some("CUSTOM_API_KEY"),
        )
        .expect("write config");
        with_home_and_env(tmp.path(), &[("CUSTOM_API_KEY", "custom-env-key")], || {
            assert!(summarize_api_key_configured());
        });

        with_home_and_env(tmp.path(), &[("OPENAI_API_KEY", "env-key")], || {
            assert!(
                !summarize_api_key_configured(),
                "apiKeyEnv in config must be satisfied before OPENAI_API_KEY fallback"
            );
        });

        with_home_and_env(tmp.path(), &[], || {
            assert!(!summarize_api_key_configured());
        });

        let tmp_openai = tempfile::tempdir().expect("tempdir");
        with_home_and_env(tmp_openai.path(), &[("OPENAI_API_KEY", "env-key")], || {
            assert!(summarize_api_key_configured());
        });
    }

    #[test]
    fn load_mmr_config_parses_camel_case_summarize_fields() {
        let tmp = tempfile::tempdir().expect("tempdir");
        write_summarize_config_for_tests_with_api(
            tmp.path(),
            "http://cfg/v1",
            "gpt-5.5",
            Some("inline-key"),
            Some("OPENAI_API_KEY"),
        )
        .expect("write config");
        with_home_and_env(tmp.path(), &[], || {
            let config = load_mmr_config().expect("load").expect("file");
            let summarize = config.summarize.expect("summarize section");
            assert_eq!(summarize.api_key.as_deref(), Some("inline-key"));
            assert_eq!(summarize.api_key_env.as_deref(), Some("OPENAI_API_KEY"));
            assert_eq!(summarize.base_url.as_deref(), Some("http://cfg/v1"));
            assert_eq!(summarize.model.as_deref(), Some("gpt-5.5"));
        });
    }

    #[test]
    fn resolve_cli_model_overrides_config_and_env() {
        let tmp = tempfile::tempdir().expect("tempdir");
        write_summarize_config_for_tests(tmp.path(), "http://cfg/v1", "gpt-5.5")
            .expect("write config");
        with_home_and_env(
            tmp.path(),
            &[
                ("OPENAI_API_KEY", "env-key"),
                ("MMR_SUMMARISER_MODEL", "env-model"),
            ],
            || {
                let resolved = resolve_summarize_settings(Some("gpt-5.4")).expect("resolve");
                assert_eq!(resolved.model, "gpt-5.4");
            },
        );
    }

    fn mmr_config_path_from_home(home: &Path) -> Option<PathBuf> {
        let _lock = env_test_lock();
        unsafe {
            std::env::set_var("HOME", home);
            std::env::remove_var(ENV_CONFIG_FILE);
            std::env::remove_var("XDG_CONFIG_HOME");
        }
        mmr_config_path()
    }

    fn with_home_and_env<F: FnOnce()>(home: &Path, env: &[(&str, &str)], f: F) {
        let _lock = env_test_lock();
        let isolated_keys = [
            ENV_OPENAI_API_KEY,
            ENV_OPENAI_BASE_URL,
            ENV_SUMMARISER_MODEL,
            "HOME",
            "XDG_CONFIG_HOME",
            ENV_CONFIG_FILE,
        ];
        let mut prior_env: Vec<(String, Option<String>)> = isolated_keys
            .iter()
            .map(|key| ((*key).to_string(), std::env::var(key).ok()))
            .collect();
        for (key, _) in env {
            if !prior_env.iter().any(|(name, _)| name == key) {
                prior_env.push(((*key).to_string(), std::env::var(key).ok()));
            }
        }
        unsafe {
            std::env::set_var("HOME", home);
            std::env::remove_var("XDG_CONFIG_HOME");
            std::env::remove_var(ENV_CONFIG_FILE);
            std::env::remove_var(ENV_OPENAI_API_KEY);
            std::env::remove_var(ENV_OPENAI_BASE_URL);
            std::env::remove_var(ENV_SUMMARISER_MODEL);
            for (key, value) in env {
                std::env::set_var(key, value);
            }
        }
        f();
        for (key, prior) in prior_env {
            restore_env(&key, prior);
        }
    }

    fn restore_env(key: &str, prior: Option<String>) {
        unsafe {
            match prior {
                Some(value) => std::env::set_var(key, value),
                None => std::env::remove_var(key),
            }
        }
    }
}
