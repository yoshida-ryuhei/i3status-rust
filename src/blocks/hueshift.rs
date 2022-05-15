use super::prelude::*;

pub struct Hueshift {
    text: TextWidget,
    temp: u16,
    // update_interval: Option<Duration>,
    step: u16,
    max_temp: u16,
    min_temp: u16,
    hue_shift_driver: Box<dyn HueShiftDriver>,
    click_temp: u16,
}

#[derive(Deserialize, Debug, Clone, SmartDefault)]
#[serde(deny_unknown_fields, default)]
pub struct HueshiftConfig {
    // interval: Option<Seconds>,
    #[default(10_000)]
    max_temp: u16,
    #[default(1_000)]
    min_temp: u16,
    #[default(6_500)]
    current_temp: u16,
    hue_shifter: Option<HueShifter>,
    #[default(100)]
    step: u16,
    #[default(6_500)]
    click_temp: u16,
}

#[derive(Deserialize, Debug, Clone)]
#[serde(rename_all = "lowercase")]
pub enum HueShifter {
    Redshift,
    Sct,
    Gammastep,
    Wlsunset,
    WlGammarelay,
    WlGammarelayRs,
}

#[async_trait]
impl ConfigBlock for Hueshift {
    type Config = HueshiftConfig;

    async fn new(
        id: usize,
        config: Self::Config,
        shared_config: SharedConfig,
        update_request: Sender<usize>,
    ) -> Result<Self> {
        let hue_shifter = config
            .hue_shifter
            .or_else(|| {
                if has_command("redshift").unwrap_or(false) {
                    Some(HueShifter::Redshift)
                } else if has_command("sct").unwrap_or(false) {
                    Some(HueShifter::Sct)
                } else if has_command("gammastep").unwrap_or(false) {
                    Some(HueShifter::Gammastep)
                } else if has_command("wlsunset").unwrap_or(false) {
                    Some(HueShifter::Wlsunset)
                } else if has_command("wl-gammarelay-rs").unwrap_or(false) {
                    Some(HueShifter::WlGammarelayRs)
                } else if has_command("wl-gammarelay").unwrap_or(false) {
                    Some(HueShifter::WlGammarelay)
                } else {
                    None
                }
            })
            .error_msg("Cound not detect driver program")?;

        let hue_shift_driver: Box<dyn HueShiftDriver> = match hue_shifter {
            HueShifter::Redshift => Box::new(Redshift),
            HueShifter::Sct => Box::new(Sct),
            HueShifter::Gammastep => Box::new(Gammastep),
            HueShifter::Wlsunset => Box::new(Wlsunset),
            HueShifter::WlGammarelayRs => {
                Box::new(WlGammarelay::new("wl-gammarelay-rs", id, update_request).await?)
            }
            HueShifter::WlGammarelay => {
                Box::new(WlGammarelay::new("wl-gammarelay", id, update_request).await?)
            }
        };

        Ok(Hueshift {
            // update_interval: config.interval.map(|i| i.0),
            temp: config.current_temp,
            step: config.step.max(500),
            max_temp: config.max_temp.clamp(1_000, 10_000),
            min_temp: config.min_temp.clamp(1_000, config.max_temp),
            hue_shift_driver,
            click_temp: config.click_temp,
            text: TextWidget::new(id, 0, shared_config).with_text(&config.current_temp.to_string()),
        })
    }
}

#[async_trait]
impl Block for Hueshift {
    // fn interval(&self) -> Option<Duration> {
    // If drivers have a way of polling for the current temperature then it
    // makes sense to have an update interval otherwise it has no effect.
    // None of the drivers besides WlGammarelay has a mechanism to get the
    // current temperature if they are changed outside of the statusbar.
    // Although WlGammarelay can get the current temperature it doesn't need
    // to run update on an update interval as it is listening to dbus events.
    // self.update_interval
    // }

    async fn update(&mut self) -> Result<()> {
        if let Some(current_temp) = self.hue_shift_driver.get_current_temperature().await? {
            self.temp = current_temp;
        }
        self.text.set_text(self.temp.to_string());
        Ok(())
    }

    async fn click(&mut self, event: &I3BarEvent) -> Result<bool> {
        let mut to_update = true;
        match event.button {
            MouseButton::Left => {
                self.temp = self.click_temp;
                self.hue_shift_driver.update(self.temp).await?;
            }
            MouseButton::Right => {
                self.temp = 6500;
                self.hue_shift_driver.reset().await?;
            }
            MouseButton::WheelUp if self.temp < self.max_temp => {
                self.temp = (self.temp + self.step).min(self.max_temp);
                self.hue_shift_driver.update(self.temp).await?;
            }
            MouseButton::WheelDown if self.temp > self.min_temp => {
                self.temp = self.temp.saturating_sub(self.step).max(self.min_temp);
                self.hue_shift_driver.update(self.temp).await?;
            }
            _ => to_update = false,
        }
        Ok(to_update)
    }

    fn view(&self) -> Vec<&dyn I3BarWidget> {
        vec![&self.text]
    }
}

#[async_trait]
trait HueShiftDriver: Send + Sync {
    async fn update(&self, temp: u16) -> Result<()>;

    async fn reset(&self) -> Result<()>;

    async fn get_current_temperature(&self) -> Result<Option<u16>> {
        Ok(None)
    }
}

struct Redshift;

#[async_trait]
impl HueShiftDriver for Redshift {
    async fn update(&self, temp: u16) -> Result<()> {
        spawn_child_async(
            "sh",
            &[
                "-c",
                format!("redshift -O {} -P >/dev/null 2>&1", temp).as_str(),
            ],
        )
        .error_msg("Failed to set new color temperature using redshift")?;
        Ok(())
    }
    async fn reset(&self) -> Result<()> {
        spawn_child_async("sh", &["-c", "redshift -x >/dev/null 2>&1"])
            .error_msg("Failed to set new color temperature using redshift")?;
        Ok(())
    }
}

struct Sct;

#[async_trait]
impl HueShiftDriver for Sct {
    async fn update(&self, temp: u16) -> Result<()> {
        spawn_child_async(
            "sh",
            &["-c", format!("sct {} >/dev/null 2>&1", temp).as_str()],
        )
        .error_msg("Failed to set new color temperature using sct")?;
        Ok(())
    }
    async fn reset(&self) -> Result<()> {
        spawn_child_async("sh", &["-c", "sct >/dev/null 2>&1"])
            .error_msg("Failed to set new color temperature using sct")?;
        Ok(())
    }
}

struct Gammastep;

#[async_trait]
impl HueShiftDriver for Gammastep {
    async fn update(&self, temp: u16) -> Result<()> {
        spawn_child_async(
            "sh",
            &[
                "-c",
                &format!("pkill gammastep; gammastep -O {} -P &", temp),
            ],
        )
        .error_msg("Failed to set new color temperature using gammastep")
    }
    async fn reset(&self) -> Result<()> {
        spawn_child_async("sh", &["-c", "gammastep -x >/dev/null 2>&1"])
            .error_msg("Failed to set new color temperature using gammastep")
    }
}

struct Wlsunset;

#[async_trait]
impl HueShiftDriver for Wlsunset {
    async fn update(&self, temp: u16) -> Result<()> {
        // wlsunset does not have a oneshot option, so set both day and
        // night temperature. wlsunset dose not allow for day and night
        // temperatures to be the same, so increment the day temperature.
        spawn_child_async(
            "sh",
            &[
                "-c",
                &format!("pkill wlsunset; wlsunset -T {} -t {} &", temp + 1, temp),
            ],
        )
        .error_msg("Failed to set new color temperature using wlsunset")?;
        Ok(())
    }
    async fn reset(&self) -> Result<()> {
        // wlsunset does not have a reset option, so just kill the process.
        // Trying to call wlsunset without any arguments uses the defaults:
        // day temp: 6500K
        // night temp: 4000K
        // latitude/longitude: NaN
        //     ^ results in sun_condition == POLAR_NIGHT at time of testing
        // With these defaults, this results in the the color temperature
        // getting set to 4000K.
        spawn_child_async("sh", &["-c", "pkill wlsunset > /dev/null 2>&1"])
            .error_msg("Failed to set new color temperature using wlsunset")?;
        Ok(())
    }
}

struct WlGammarelay {
    proxy: WlGammarelayRsBusProxy<'static>,
}

impl WlGammarelay {
    async fn new(cmd: &str, id: usize, sender: Sender<usize>) -> Result<Self> {
        spawn_child_async(cmd, &[]).error_msg("Failed to start wl-gammarelay daemon")?;
        tokio::time::sleep(Duration::from_millis(200)).await;

        let conn = new_dbus_connection().await?;
        let proxy = WlGammarelayRsBusProxy::new(&conn)
            .await
            .error_msg("Failed to create wl-gammarelay-rs DBus proxy")?;

        {
            let proxy = proxy.clone();
            tokio::spawn(async move {
                let mut updates = proxy.receive_temperature_changed().await;
                while updates.next().await.is_some() {
                    if sender.send(id).await.is_err() {
                        break;
                    }
                }
                Ok::<_, Error>(())
            });
        }

        Ok(Self { proxy })
    }
}

#[async_trait]
impl HueShiftDriver for WlGammarelay {
    async fn update(&self, temp: u16) -> Result<()> {
        self.proxy
            .set_temperature(temp)
            .await
            .error_msg("could not update temperature")
    }

    async fn reset(&self) -> Result<()> {
        self.update(6500).await
    }

    async fn get_current_temperature(&self) -> Result<Option<u16>> {
        Ok(Some(
            self.proxy
                .temperature()
                .await
                .error_msg("could not get temperature")?,
        ))
    }
}

#[zbus::dbus_proxy(
    interface = "rs.wl.gammarelay",
    default_service = "rs.wl-gammarelay",
    default_path = "/"
)]
trait WlGammarelayRsBus {
    /// Temperature property
    #[dbus_proxy(property)]
    fn temperature(&self) -> zbus::Result<u16>;
    #[dbus_proxy(property)]
    fn set_temperature(&self, value: u16) -> zbus::Result<()>;
}
