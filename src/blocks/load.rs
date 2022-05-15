use super::prelude::*;
use tokio::fs::read_to_string;

pub struct Load {
    text: TextWidget,
    logical_cores: u32,
    format: FormatTemplate,
    update_interval: Duration,
    minimum_info: f64,
    minimum_warning: f64,
    minimum_critical: f64,
}

#[derive(Deserialize, Debug, Clone, SmartDefault)]
#[serde(deny_unknown_fields, default)]
pub struct LoadConfig {
    format: FormatTemplate,
    #[default(5.into())]
    interval: Seconds,
    #[default(0.3)]
    info: f64,
    #[default(0.6)]
    warning: f64,
    #[default(0.9)]
    critical: f64,
}

#[async_trait]
impl ConfigBlock for Load {
    type Config = LoadConfig;

    async fn new(
        id: usize,
        config: Self::Config,
        shared_config: SharedConfig,
        _: Sender<usize>,
    ) -> Result<Self> {
        let text = TextWidget::new(id, 0, shared_config)
            .with_icon("cogs")?
            .with_state(State::Info);

        // borrowed from https://docs.rs/cpuinfo/0.1.1/src/cpuinfo/count/logical.rs.html#4-6
        let content = read_to_string("/proc/cpuinfo")
            .await
            .error_msg("Your system doesn't support /proc/cpuinfo")?;
        let logical_cores = content
            .lines()
            .filter(|l| l.starts_with("processor"))
            .count() as u32;

        Ok(Load {
            logical_cores,
            update_interval: config.interval.0,
            minimum_info: config.info,
            minimum_warning: config.warning,
            minimum_critical: config.critical,
            format: config.format.with_default("{1m}")?,
            text,
        })
    }
}

#[async_trait]
impl Block for Load {
    fn interval(&self) -> Option<Duration> {
        Some(self.update_interval)
    }

    async fn update(&mut self) -> Result<()> {
        let loadavg = read_to_string("/proc/loadavg").await.error_msg(
            "Your system does not support reading the load average from /proc/loadavg",
        )?;

        let split: Vec<f64> = loadavg
            .split(' ')
            .take(3)
            .map(|x| x.parse().unwrap())
            .collect();

        let values = map!(
            "1m" => Value::from_float(split[0]),
            "5m" => Value::from_float(split[1]),
            "15m" => Value::from_float(split[2]),
        );

        let used_perc = split[0] / (self.logical_cores as f64);

        self.text.set_state(match used_perc {
            x if x > self.minimum_critical => State::Critical,
            x if x > self.minimum_warning => State::Warning,
            x if x > self.minimum_info => State::Info,
            _ => State::Idle,
        });

        self.text.set_texts(self.format.render(&values)?);

        Ok(())
    }

    fn view(&self) -> Vec<&dyn I3BarWidget> {
        vec![&self.text]
    }
}
