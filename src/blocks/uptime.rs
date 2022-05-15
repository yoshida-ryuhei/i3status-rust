use super::prelude::*;

pub struct Uptime {
    text: TextWidget,
    update_interval: Duration,
}

#[derive(Deserialize, Debug, Clone, SmartDefault)]
#[serde(deny_unknown_fields, default)]
pub struct UptimeConfig {
    #[default(60.into())]
    interval: Seconds,
}

#[async_trait]
impl ConfigBlock for Uptime {
    type Config = UptimeConfig;

    async fn new(
        id: usize,
        config: Self::Config,
        shared_config: SharedConfig,
        _: Sender<usize>,
    ) -> Result<Self> {
        Ok(Uptime {
            text: TextWidget::new(id, 0, shared_config).with_icon("uptime")?,
            update_interval: config.interval.0,
        })
    }
}

#[async_trait]
impl Block for Uptime {
    fn interval(&self) -> Option<Duration> {
        Some(self.update_interval)
    }

    async fn update(&mut self) -> Result<()> {
        let uptime_raw = read_file("/proc/uptime")
            .await
            .error_msg("Uptime failed to read /proc/uptime")?;
        let uptime = uptime_raw
            .split_whitespace()
            .next()
            .error_msg("Uptime failed to read uptime string.")?;

        let total_seconds = uptime
            .parse::<f64>()
            .map(|x| x as u32)
            .error_msg("Failed to convert uptime float to integer)")?;

        // split up seconds into more human readable portions
        let weeks = (total_seconds / 604_800) as u32;
        let rem_weeks = (total_seconds % 604_800) as u32;
        let days = (rem_weeks / 86_400) as u32;
        let rem_days = (rem_weeks % 86_400) as u32;
        let hours = (rem_days / 3600) as u32;
        let rem_hours = (rem_days % 3600) as u32;
        let minutes = (rem_hours / 60) as u32;
        let rem_minutes = (rem_hours % 60) as u32;
        let seconds = rem_minutes as u32;

        // Display the two largest units.
        self.text
            .set_text(if hours == 0 && days == 0 && weeks == 0 {
                format!("{}m {}s", minutes, seconds)
            } else if hours > 0 && days == 0 && weeks == 0 {
                format!("{}h {}m", hours, minutes)
            } else if days > 0 && weeks == 0 {
                format!("{}d {}h", days, hours)
            } else if days == 0 && weeks > 0 {
                format!("{}w {}h", weeks, hours)
            } else if weeks > 0 {
                format!("{}w {}d", weeks, days)
            } else {
                unreachable!()
            });
        Ok(())
    }

    fn view(&self) -> Vec<&dyn I3BarWidget> {
        vec![&self.text]
    }
}
