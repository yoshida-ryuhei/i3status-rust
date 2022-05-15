use super::prelude::*;
use crate::formatting::{prefix::Prefix, value::Value};
use nix::sys::statvfs::statvfs;
use std::path::Path;
use std::time::Duration;

#[derive(Copy, Clone, Debug, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum InfoType {
    Available,
    Free,
    Used,
}

pub struct DiskSpace {
    text: TextWidget,
    update_interval: Duration,
    path: String,
    unit: Prefix,
    info_type: InfoType,
    warning: f64,
    alert: f64,
    alert_absolute: bool,
    format: FormatTemplate,
    icon: String,
}

#[derive(Deserialize, Debug, Clone, SmartDefault)]
#[serde(deny_unknown_fields, default)]
pub struct DiskSpaceConfig {
    #[default("/".into())]
    path: ShellString,
    #[default(InfoType::Available)]
    info_type: InfoType,
    format: FormatTemplate,
    #[default("GB".into())]
    unit: String,
    #[default(20.into())]
    interval: Seconds,
    #[default(20.0)]
    warning: f64,
    #[default(10.0)]
    alert: f64,
    alert_absolute: bool,
}

enum AlertType {
    Above,
    Below,
}

impl DiskSpace {
    fn compute_state(&self, value: f64, warning: f64, alert: f64, alert_type: AlertType) -> State {
        match alert_type {
            AlertType::Above => {
                if value > alert {
                    State::Critical
                } else if value <= alert && value > warning {
                    State::Warning
                } else {
                    State::Idle
                }
            }
            AlertType::Below => {
                if 0. <= value && value < alert {
                    State::Critical
                } else if alert <= value && value < warning {
                    State::Warning
                } else {
                    State::Idle
                }
            }
        }
    }
}

#[async_trait]
impl ConfigBlock for DiskSpace {
    type Config = DiskSpaceConfig;

    async fn new(
        id: usize,
        config: Self::Config,
        shared_config: SharedConfig,
        _: Sender<usize>,
    ) -> Result<Self> {
        Ok(DiskSpace {
            icon: shared_config.get_icon("disk_drive")?.trim().to_string(),
            update_interval: config.interval.0,
            text: TextWidget::new(id, 0, shared_config),
            path: config.path.expand()?.into(),
            format: config.format.with_default("{available}")?,
            info_type: config.info_type,
            unit: match config.unit.as_str() {
                "TB" => Prefix::Tera,
                "GB" => Prefix::Giga,
                "MB" => Prefix::Mega,
                "KB" => Prefix::Kilo,
                "B" => Prefix::One,
                x => return Err(Error::new(format!("unknown unit '{x}'"))),
            },
            warning: config.warning,
            alert: config.alert,
            alert_absolute: config.alert_absolute,
        })
    }
}

#[async_trait]
impl Block for DiskSpace {
    fn interval(&self) -> Option<Duration> {
        Some(self.update_interval)
    }

    async fn update(&mut self) -> Result<()> {
        let statvfs =
            statvfs(Path::new(self.path.as_str())).error_msg("failed to retrieve statvfs")?;

        let total = (statvfs.blocks() as u64) * (statvfs.fragment_size() as u64);
        let used = ((statvfs.blocks() as u64) - (statvfs.blocks_free() as u64))
            * (statvfs.fragment_size() as u64);
        let available = (statvfs.blocks_available() as u64) * (statvfs.block_size() as u64);
        let free = (statvfs.blocks_free() as u64) * (statvfs.block_size() as u64);

        let result;
        let alert_type;
        match self.info_type {
            InfoType::Available => {
                result = available as f64;
                alert_type = AlertType::Below;
            }
            InfoType::Free => {
                result = free as f64;
                alert_type = AlertType::Below;
            }
            InfoType::Used => {
                result = used as f64;
                alert_type = AlertType::Above;
            }
        }

        let percentage = result / (total as f64) * 100.;
        self.text.set_texts(self.format.render(&map! {
            "percentage" => Value::from_float(percentage).percents(),
            "path" => Value::from_string(self.path.clone()),
            "total" => Value::from_float(total as f64).bytes(),
            "used" => Value::from_float(used as f64).bytes(),
            "available" => Value::from_float(available as f64).bytes(),
            "free" => Value::from_float(free as f64).bytes(),
            "icon" => Value::from_string(self.icon.to_string()),
        })?);

        // Send percentage to alert check if we don't want absolute alerts
        let alert_val = if self.alert_absolute {
            result
                / match self.unit {
                    Prefix::Tera => 1u64 << 40,
                    Prefix::Giga => 1u64 << 30,
                    Prefix::Mega => 1u64 << 20,
                    Prefix::Kilo => 1u64 << 10,
                    Prefix::One => 1u64,
                    _ => unreachable!(),
                } as f64
        } else {
            percentage
        };
        self.text
            .set_state(self.compute_state(alert_val, self.warning, self.alert, alert_type));

        Ok(())
    }

    fn view(&self) -> Vec<&dyn I3BarWidget> {
        vec![&self.text]
    }
}
