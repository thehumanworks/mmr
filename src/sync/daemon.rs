use std::fs;
use std::path::Path;

use anyhow::{Context, Result};

use crate::sync::config::SyncConfig;
use crate::sync::{Platform, detect_platform};

const PLIST_LABEL: &str = "com.mmr.sync";
const SYSTEMD_SERVICE: &str = "mmr-sync.service";
const SYSTEMD_TIMER: &str = "mmr-sync.timer";
const WRAPPER_SCRIPT: &str = "sync-daemon.sh";

pub fn install(interval: u32) -> Result<String> {
    let config = SyncConfig::load()?;
    let platform = detect_platform();

    match platform {
        Platform::MacOS => install_launchd(interval, &config),
        Platform::Linux => install_systemd(interval, &config),
        Platform::Unsupported => {
            anyhow::bail!("daemon install is only supported on macOS and Linux")
        }
    }
}

pub fn uninstall() -> Result<String> {
    let platform = detect_platform();

    match platform {
        Platform::MacOS => uninstall_launchd(),
        Platform::Linux => uninstall_systemd(),
        Platform::Unsupported => {
            anyhow::bail!("daemon uninstall is only supported on macOS and Linux")
        }
    }
}

// --- macOS launchd ---

fn install_launchd(interval: u32, config: &SyncConfig) -> Result<String> {
    let mmr_bin = find_mmr_binary()?;
    let config_dir = SyncConfig::config_dir()?;
    fs::create_dir_all(&config_dir)?;

    // Write wrapper script
    let script_path = config_dir.join(WRAPPER_SCRIPT);
    let script_content = generate_wrapper_script(&mmr_bin, config);
    fs::write(&script_path, &script_content)?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&script_path, fs::Permissions::from_mode(0o755))?;
    }

    // Write plist
    let home = dirs::home_dir().context("cannot find home directory")?;
    let plist_dir = home.join("Library/LaunchAgents");
    fs::create_dir_all(&plist_dir)?;
    let plist_path = plist_dir.join(format!("{}.plist", PLIST_LABEL));
    let plist_content = generate_plist(&script_path, interval, &config_dir);
    fs::write(&plist_path, &plist_content)?;

    // Load the agent
    let output = std::process::Command::new("launchctl")
        .args(["load", &plist_path.to_string_lossy()])
        .output()
        .context("failed to run launchctl load")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("launchctl load failed: {}", stderr);
    }

    Ok(format!(
        "Installed macOS LaunchAgent.\n\
         Plist: {}\n\
         Script: {}\n\
         Interval: {}s ({}min)\n\
         Quiet hours: {}-{} {}",
        plist_path.display(),
        script_path.display(),
        interval * 60,
        interval,
        config.sync.quiet_start,
        config.sync.quiet_end,
        config.sync.quiet_timezone,
    ))
}

fn uninstall_launchd() -> Result<String> {
    let home = dirs::home_dir().context("cannot find home directory")?;
    let plist_path = home.join(format!("Library/LaunchAgents/{}.plist", PLIST_LABEL));

    if plist_path.exists() {
        let _ = std::process::Command::new("launchctl")
            .args(["unload", &plist_path.to_string_lossy()])
            .output();
        fs::remove_file(&plist_path)?;
    }

    let config_dir = SyncConfig::config_dir()?;
    let script_path = config_dir.join(WRAPPER_SCRIPT);
    if script_path.exists() {
        fs::remove_file(&script_path)?;
    }

    Ok("Uninstalled macOS LaunchAgent.".to_string())
}

fn generate_plist(script_path: &Path, interval: u32, log_dir: &Path) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>{label}</string>
    <key>ProgramArguments</key>
    <array>
        <string>/bin/bash</string>
        <string>{script}</string>
    </array>
    <key>StartInterval</key>
    <integer>{interval_secs}</integer>
    <key>StandardOutPath</key>
    <string>{log_dir}/sync.log</string>
    <key>StandardErrorPath</key>
    <string>{log_dir}/sync.err</string>
    <key>RunAtLoad</key>
    <false/>
</dict>
</plist>"#,
        label = PLIST_LABEL,
        script = script_path.display(),
        interval_secs = interval * 60,
        log_dir = log_dir.display(),
    )
}

// --- Linux systemd ---

fn install_systemd(interval: u32, config: &SyncConfig) -> Result<String> {
    let mmr_bin = find_mmr_binary()?;
    let config_dir = SyncConfig::config_dir()?;
    fs::create_dir_all(&config_dir)?;

    // Write wrapper script
    let script_path = config_dir.join(WRAPPER_SCRIPT);
    let script_content = generate_wrapper_script(&mmr_bin, config);
    fs::write(&script_path, &script_content)?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&script_path, fs::Permissions::from_mode(0o755))?;
    }

    // Write systemd units
    let home = dirs::home_dir().context("cannot find home directory")?;
    let systemd_dir = home.join(".config/systemd/user");
    fs::create_dir_all(&systemd_dir)?;

    let service_path = systemd_dir.join(SYSTEMD_SERVICE);
    let service_content = generate_systemd_service(&script_path);
    fs::write(&service_path, &service_content)?;

    let timer_path = systemd_dir.join(SYSTEMD_TIMER);
    let timer_content = generate_systemd_timer(interval);
    fs::write(&timer_path, &timer_content)?;

    // Reload and enable
    run_systemctl(&["daemon-reload"])?;
    run_systemctl(&["enable", "--now", SYSTEMD_TIMER])?;

    Ok(format!(
        "Installed systemd user timer.\n\
         Service: {}\n\
         Timer: {}\n\
         Script: {}\n\
         Interval: {}min\n\
         Quiet hours: {}-{} {}",
        service_path.display(),
        timer_path.display(),
        script_path.display(),
        interval,
        config.sync.quiet_start,
        config.sync.quiet_end,
        config.sync.quiet_timezone,
    ))
}

fn uninstall_systemd() -> Result<String> {
    let _ = run_systemctl(&["disable", "--now", SYSTEMD_TIMER]);

    let home = dirs::home_dir().context("cannot find home directory")?;
    let systemd_dir = home.join(".config/systemd/user");

    for file in &[SYSTEMD_SERVICE, SYSTEMD_TIMER] {
        let path = systemd_dir.join(file);
        if path.exists() {
            fs::remove_file(&path)?;
        }
    }

    let _ = run_systemctl(&["daemon-reload"]);

    let config_dir = SyncConfig::config_dir()?;
    let script_path = config_dir.join(WRAPPER_SCRIPT);
    if script_path.exists() {
        fs::remove_file(&script_path)?;
    }

    Ok("Uninstalled systemd user timer.".to_string())
}

fn generate_systemd_service(script_path: &Path) -> String {
    format!(
        "[Unit]\n\
         Description=mmr conversation history sync\n\
         \n\
         [Service]\n\
         Type=oneshot\n\
         ExecStart=/bin/bash {script}\n\
         \n\
         [Install]\n\
         WantedBy=default.target\n",
        script = script_path.display(),
    )
}

fn generate_systemd_timer(interval: u32) -> String {
    format!(
        "[Unit]\n\
         Description=mmr sync timer\n\
         \n\
         [Timer]\n\
         OnBootSec=5min\n\
         OnUnitActiveSec={interval}min\n\
         Persistent=true\n\
         \n\
         [Install]\n\
         WantedBy=timers.target\n",
        interval = interval,
    )
}

fn run_systemctl(args: &[&str]) -> Result<()> {
    let output = std::process::Command::new("systemctl")
        .arg("--user")
        .args(args)
        .output()
        .context("failed to run systemctl")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("systemctl --user {} failed: {}", args.join(" "), stderr);
    }
    Ok(())
}

// --- Shared ---

fn generate_wrapper_script(mmr_bin: &str, config: &SyncConfig) -> String {
    format!(
        r#"#!/bin/bash
# mmr sync daemon wrapper
# Checks quiet hours before running sync push

LONDON_HOUR=$(TZ={timezone} date +%H)
QUIET_START={quiet_start_hour}
QUIET_END={quiet_end_hour}

if [ "$LONDON_HOUR" -ge "$QUIET_START" ] && [ "$LONDON_HOUR" -lt "$QUIET_END" ]; then
    echo "$(date -u +%Y-%m-%dT%H:%M:%SZ) Quiet hours ($QUIET_START:00-$QUIET_END:00 {timezone}), skipping sync"
    exit 0
fi

echo "$(date -u +%Y-%m-%dT%H:%M:%SZ) Starting sync push..."
{mmr_bin} sync push
EXIT_CODE=$?
echo "$(date -u +%Y-%m-%dT%H:%M:%SZ) Sync push finished (exit code: $EXIT_CODE)"
exit $EXIT_CODE
"#,
        timezone = config.sync.quiet_timezone,
        quiet_start_hour = parse_hour(&config.sync.quiet_start),
        quiet_end_hour = parse_hour(&config.sync.quiet_end),
        mmr_bin = mmr_bin,
    )
}

fn parse_hour(time_str: &str) -> u32 {
    time_str
        .split(':')
        .next()
        .and_then(|h| h.parse().ok())
        .unwrap_or(0)
}

fn find_mmr_binary() -> Result<String> {
    std::env::current_exe()
        .context("could not determine mmr binary path")
        .map(|p| p.to_string_lossy().into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn parse_hour_extracts_correctly() {
        assert_eq!(parse_hour("03:00"), 3);
        assert_eq!(parse_hour("09:00"), 9);
        assert_eq!(parse_hour("23:30"), 23);
        assert_eq!(parse_hour("invalid"), 0);
    }

    #[test]
    fn wrapper_script_contains_quiet_hours() {
        let config = SyncConfig {
            storage: crate::sync::config::StorageConfig {
                provider: "r2".to_string(),
                endpoint: "https://test.r2.cloudflarestorage.com".to_string(),
                bucket: "b".to_string(),
                access_key_id: "k".to_string(),
                secret_access_key: "s".to_string(),
                region: "auto".to_string(),
            },
            sync: crate::sync::config::ScheduleConfig {
                quiet_start: "03:00".to_string(),
                quiet_end: "09:00".to_string(),
                quiet_timezone: "Europe/London".to_string(),
                interval_minutes: 15,
            },
            sources: Default::default(),
        };
        let script = generate_wrapper_script("/usr/local/bin/mmr", &config);
        assert!(script.contains("Europe/London"));
        assert!(script.contains("QUIET_START=3"));
        assert!(script.contains("QUIET_END=9"));
        assert!(script.contains("/usr/local/bin/mmr sync push"));
    }

    #[test]
    fn plist_has_correct_interval() {
        let plist = generate_plist(
            &PathBuf::from("/tmp/script.sh"),
            15,
            &PathBuf::from("/tmp/logs"),
        );
        assert!(plist.contains("<integer>900</integer>")); // 15 * 60
        assert!(plist.contains(PLIST_LABEL));
        assert!(plist.contains("/tmp/script.sh"));
    }

    #[test]
    fn systemd_timer_has_correct_interval() {
        let timer = generate_systemd_timer(15);
        assert!(timer.contains("OnUnitActiveSec=15min"));
        assert!(timer.contains("Persistent=true"));
    }

    #[test]
    fn systemd_service_references_script() {
        let service =
            generate_systemd_service(&PathBuf::from("/home/user/.config/mmr/sync-daemon.sh"));
        assert!(service.contains("ExecStart=/bin/bash /home/user/.config/mmr/sync-daemon.sh"));
        assert!(service.contains("Type=oneshot"));
    }
}
