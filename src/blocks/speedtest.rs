use super::prelude::*;

pub struct SpeedTest {
    text: TextWidget,
    format: FormatTemplate,
    interval: Duration,
    ping_icon: String,
    down_icon: String,
    up_icon: String,
}

#[derive(Deserialize, Debug, Clone, SmartDefault)]
#[serde(deny_unknown_fields, default)]
pub struct SpeedTestConfig {
    format: FormatTemplate,
    #[default(1800.into())]
    interval: Seconds,
}

#[async_trait]
impl ConfigBlock for SpeedTest {
    type Config = SpeedTestConfig;

    async fn new(
        id: usize,
        block_config: Self::Config,
        shared_config: SharedConfig,
        _: Sender<usize>,
    ) -> Result<Self> {
        Ok(SpeedTest {
            format: block_config
                .format
                .with_default("{ping}{speed_down}{speed_up}")?,
            interval: block_config.interval.0,
            ping_icon: shared_config.get_icon("ping")?,
            down_icon: shared_config.get_icon("net_down")?,
            up_icon: shared_config.get_icon("net_up")?,
            text: TextWidget::new(id, 0, shared_config).with_text("..."),
        })
    }
}

#[async_trait]
impl Block for SpeedTest {
    fn interval(&self) -> Option<Duration> {
        Some(self.interval)
    }

    async fn update(&mut self) -> Result<()> {
        let stats = Stats::new().await?;

        self.text.set_texts(self.format.render(&map! {
            "ping" => Value::from_float(stats.ping * 1e-3).seconds().icon(self.ping_icon.clone()),
            "speed_down" => Value::from_float(stats.download).bits().icon(self.down_icon.clone()),
            "speed_up" => Value::from_float(stats.upload).bits().icon(self.up_icon.clone()),
        })?);

        Ok(())
    }

    async fn click(&mut self, e: &I3BarEvent) -> Result<bool> {
        Ok(e.button == MouseButton::Left)
    }

    fn view(&self) -> Vec<&dyn I3BarWidget> {
        vec![&self.text]
    }
}

#[derive(Deserialize, Debug, Clone, Copy)]
struct Stats {
    /// Download speed in bits per second
    download: f64,
    /// Upload speed in bits per second
    upload: f64,
    /// Ping time in ms
    ping: f64,
}

impl Stats {
    async fn new() -> Result<Self> {
        let output = Command::new("speedtest-cli")
            .arg("--json")
            .output()
            .await
            .error_msg("could not get speedtest-cli output")?
            .stdout;
        serde_json::from_slice(&output).error_msg("could not parse speedtest-cli json")
    }
}
