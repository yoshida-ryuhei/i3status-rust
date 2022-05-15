use sensors::FeatureType::SENSORS_FEATURE_TEMP;
use sensors::Sensors;
use sensors::SubfeatureType::SENSORS_SUBFEATURE_TEMP_INPUT;

use super::prelude::*;

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, SmartDefault)]
#[serde(rename_all = "lowercase")]
enum TemperatureScale {
    #[default]
    Celsius,
    Fahrenheit,
}

#[derive(Copy, Clone, Debug, Deserialize, SmartDefault)]
#[serde(rename_all = "lowercase")]
pub enum TemperatureDriver {
    #[default]
    Sensors,
}

pub struct Temperature {
    text: TextWidget,
    collapsed: bool,
    update_interval: Duration,
    scale: TemperatureScale,
    maximum_good: f64,
    maximum_idle: f64,
    maximum_info: f64,
    maximum_warning: f64,
    format: FormatTemplate,
    chip: Option<String>,
    inputs: Option<Vec<String>>,
}

#[derive(Deserialize, Debug, Clone, SmartDefault)]
#[serde(deny_unknown_fields, default)]
pub struct TemperatureConfig {
    #[default(5.into())]
    interval: Seconds,
    #[default(true)]
    collapsed: bool,
    scale: TemperatureScale,
    good: Option<f64>,
    idle: Option<f64>,
    info: Option<f64>,
    warning: Option<f64>,
    format: FormatTemplate,
    #[serde(rename = "driver")]
    _driver: TemperatureDriver,
    chip: Option<String>,
    inputs: Option<Vec<String>>,
}

#[async_trait]
impl ConfigBlock for Temperature {
    type Config = TemperatureConfig;

    async fn new(
        id: usize,
        config: Self::Config,
        shared_config: SharedConfig,
        _: Sender<usize>,
    ) -> Result<Self> {
        Ok(Temperature {
            update_interval: config.interval.0,
            text: TextWidget::new(id, 0, shared_config)
                .with_icon("thermometer")?
                .with_spacing(if config.collapsed {
                    Spacing::Hidden
                } else {
                    Spacing::Normal
                }),
            collapsed: config.collapsed,
            scale: config.scale,
            maximum_good: config.good.unwrap_or(match config.scale {
                TemperatureScale::Celsius => 20f64,
                TemperatureScale::Fahrenheit => 68f64,
            }),
            maximum_idle: config.idle.unwrap_or(match config.scale {
                TemperatureScale::Celsius => 45f64,
                TemperatureScale::Fahrenheit => 113f64,
            }),
            maximum_info: config.info.unwrap_or(match config.scale {
                TemperatureScale::Celsius => 60f64,
                TemperatureScale::Fahrenheit => 140f64,
            }),
            maximum_warning: config.warning.unwrap_or(match config.scale {
                TemperatureScale::Celsius => 80f64,
                TemperatureScale::Fahrenheit => 176f64,
            }),
            format: config.format.with_default("{average} avg, {max} max")?,
            chip: config.chip,
            inputs: config.inputs,
        })
    }
}

#[async_trait]
impl Block for Temperature {
    fn interval(&self) -> Option<Duration> {
        Some(self.update_interval)
    }

    async fn update(&mut self) -> Result<()> {
        let mut temperatures: Vec<f64> = Vec::new();
        let sensors = Sensors::new();

        let chips = match &self.chip {
            Some(chip) => sensors
                .detected_chips(chip)
                .error_msg("Failed to create chip iterator")?,
            None => sensors.into_iter(),
        };

        for chip in chips {
            for feat in chip {
                if *feat.feature_type() != SENSORS_FEATURE_TEMP {
                    continue;
                }
                if let Some(inputs) = &self.inputs {
                    let label = feat.get_label().error_msg("Failed to get input label")?;
                    if !inputs.contains(&label) {
                        continue;
                    }
                }
                for subfeat in feat {
                    if *subfeat.subfeature_type() == SENSORS_SUBFEATURE_TEMP_INPUT {
                        if let Ok(value) = subfeat.get_value() {
                            if (-100.0..150.0).contains(&value) {
                                temperatures.push(value);
                            } else {
                                eprintln!("Temperature ({value}) outside of range ([-100, 150])",);
                            }
                        }
                    }
                }
            }
        }

        if let TemperatureScale::Fahrenheit = self.scale {
            temperatures
                .iter_mut()
                .for_each(|c| *c = *c * 9f64 / 5f64 + 32f64);
        }

        if !temperatures.is_empty() {
            let max: f64 = temperatures
                .iter()
                .cloned()
                .reduce(f64::max)
                .error_msg("failed to get max temperature")?;

            if self.collapsed {
                self.text.set_text(String::new());
            } else {
                let min: f64 = temperatures
                    .iter()
                    .cloned()
                    .reduce(f64::min)
                    .error_msg("failed to get min temperature")?;
                let avg: f64 = temperatures.iter().sum::<f64>() / temperatures.len() as f64;
                self.text.set_texts(self.format.render(&map! {
                    "average" => Value::from_float(avg).degrees(),
                    "min" => Value::from_float(min).degrees(),
                    "max" => Value::from_float(max).degrees()
                })?);
            }

            self.text.set_state(match max {
                m if m <= self.maximum_good => State::Good,
                m if m <= self.maximum_idle => State::Idle,
                m if m <= self.maximum_info => State::Info,
                m if m <= self.maximum_warning => State::Warning,
                _ => State::Critical,
            });
        }

        Ok(())
    }

    async fn click(&mut self, e: &I3BarEvent) -> Result<bool> {
        if e.button == MouseButton::Left {
            self.collapsed = !self.collapsed;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    fn view(&self) -> Vec<&dyn I3BarWidget> {
        vec![&self.text]
    }
}
