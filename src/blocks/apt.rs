use super::prelude::*;
use std::env;
use tokio::fs;
use tokio::io::AsyncWriteExt;
use tokio::process::Command;

pub struct Apt {
    output: TextWidget,
    update_interval: Duration,
    format: FormatTemplate,
    format_singular: FormatTemplate,
    format_up_to_date: FormatTemplate,
    warning_updates_regex: Option<Regex>,
    critical_updates_regex: Option<Regex>,
    config_path: String,
}

#[derive(Deserialize, Debug, Clone, SmartDefault)]
#[serde(deny_unknown_fields, default)]
pub struct AptConfig {
    #[default(600.into())]
    interval: Seconds,
    format: FormatTemplate,
    format_singular: FormatTemplate,
    format_up_to_date: FormatTemplate,
    warning_updates_regex: Option<String>,
    critical_updates_regex: Option<String>,
}

#[async_trait]
impl ConfigBlock for Apt {
    type Config = AptConfig;

    async fn new(
        id: usize,
        block_config: Self::Config,
        shared_config: SharedConfig,
        _: Sender<usize>,
    ) -> Result<Self> {
        let mut cache_dir = env::temp_dir();
        cache_dir.push("i3rs-apt");
        if !cache_dir.exists() {
            fs::create_dir(&cache_dir)
                .await
                .error_msg("Failed to create temp dir")?;
        }

        let apt_conf = format!(
            "Dir::State \"{}\";\n
             Dir::State::lists \"lists\";\n
             Dir::Cache \"{}\";\n
             Dir::Cache::srcpkgcache \"srcpkgcache.bin\";\n
             Dir::Cache::pkgcache \"pkgcache.bin\";",
            cache_dir.display(),
            cache_dir.display()
        );

        let mut config_path = cache_dir;
        config_path.push("apt.conf");
        let mut config_file = fs::File::create(&config_path)
            .await
            .error_msg("Failed to create config file")?;
        config_file
            .write_all(apt_conf.as_bytes())
            .await
            .error_msg("Failed to write to config file")?;

        Ok(Apt {
            output: TextWidget::new(id, 0, shared_config).with_icon("update")?,
            update_interval: block_config.interval.0,
            format: block_config.format.with_default("{count:1}")?,
            format_singular: block_config.format_singular.with_default("{count:1}")?,
            format_up_to_date: block_config.format_up_to_date.with_default("{count:1}")?,
            warning_updates_regex: block_config
                .warning_updates_regex
                .as_deref()
                .map(Regex::new)
                .transpose()
                .error_msg("invalid warning updates regex")?,
            critical_updates_regex: block_config
                .critical_updates_regex
                .as_deref()
                .map(Regex::new)
                .transpose()
                .error_msg("invalid critical updates regex")?,
            config_path: config_path.into_os_string().into_string().unwrap(),
        })
    }
}

fn has_warning_update(updates: &str, regex: &Regex) -> bool {
    updates.lines().filter(|line| regex.is_match(line)).count() > 0
}

fn has_critical_update(updates: &str, regex: &Regex) -> bool {
    updates.lines().filter(|line| regex.is_match(line)).count() > 0
}

async fn get_updates_list(config_path: &str) -> Result<String> {
    // Update database
    Command::new("sh")
        .env("APT_CONFIG", config_path)
        .args(&["-c", "apt update"])
        .output()
        .await
        .error_msg("Failed to run `apt update` command")?;

    String::from_utf8(
        Command::new("sh")
            .env("APT_CONFIG", config_path)
            .args(&["-c", "apt list --upgradable"])
            .output()
            .await
            .error_msg("Problem running apt command")?
            .stdout,
    )
    .error_msg("Problem capturing apt command output")
}

fn get_update_count(updates: &str) -> usize {
    updates
        .lines()
        .filter(|line| line.contains("[upgradable"))
        .count()
}

#[async_trait]
impl Block for Apt {
    fn interval(&self) -> Option<Duration> {
        Some(self.update_interval)
    }

    async fn update(&mut self) -> Result<()> {
        let (formatting_map, warning, critical, cum_count) = {
            let updates_list = get_updates_list(&self.config_path).await?;
            let count = get_update_count(&updates_list);
            let formatting_map = map!(
                "count" => Value::from_integer(count as i64)
            );

            let warning = self
                .warning_updates_regex
                .as_ref()
                .map_or(false, |regex| has_warning_update(&updates_list, regex));
            let critical = self
                .critical_updates_regex
                .as_ref()
                .map_or(false, |regex| has_critical_update(&updates_list, regex));

            (formatting_map, warning, critical, count)
        };

        self.output.set_texts(match cum_count {
            0 => self.format_up_to_date.render(&formatting_map)?,
            1 => self.format_singular.render(&formatting_map)?,
            _ => self.format.render(&formatting_map)?,
        });
        self.output.set_state(match cum_count {
            0 => State::Idle,
            _ => {
                if critical {
                    State::Critical
                } else if warning {
                    State::Warning
                } else {
                    State::Info
                }
            }
        });

        Ok(())
    }

    async fn click(&mut self, event: &I3BarEvent) -> Result<bool> {
        Ok(event.button == MouseButton::Left)
    }

    fn view(&self) -> Vec<&dyn I3BarWidget> {
        vec![&self.output]
    }
}
