//! The brightness of a backlight device
//!
//! This block reads brightness information directly from the filesystem, so it works under both
//! X11 and Wayland. The block uses `inotify` to listen for changes in the device's brightness
//! directly, so there is no need to set an update interval. This block uses DBus to set brightness
//! level using the mouse wheel.
//!
//! # Root scaling
//!
//! Some devices expose raw values that are best handled with nonlinear scaling. The human perception of lightness is close to the cube root of relative luminance, so settings for `root_scaling` between 2.4 and 3.0 are worth trying. For devices with few discrete steps this should be 1.0 (linear). More information: <https://en.wikipedia.org/wiki/Lightness>
//!
//! # Configuration
//!
//! Key | Values | Required | Default
//! ----|--------|----------|--------
//! `device` | The `/sys/class/backlight` device to read brightness information from.  When there is no `device` specified, this block will display information from the first device found in the `/sys/class/backlight` directory. If you only have one display, this approach should find it correctly.| No | Default device
//! `format` | A string to customise the output of this block. See below for available placeholders. | No | `"$brightness"`
//! `step_width` | The brightness increment to use when scrolling, in percent | No | `5`
//! `minimum` | The minimum brightness that can be scrolled down to | No | `5`
//! `maximum` | The maximum brightness that can be scrolled up to | No | `100`
//! `cycle` | The brightnesses to cycle through on each click | No | `[minimum, maximum]`
//! `root_scaling` | Scaling exponent reciprocal (ie. root) | No | `1.0`
//! `invert_icons` | Invert icons' ordering, useful if you have colorful emoji | No | `false`
//!
//! Placeholder  | Value              | Type   | Unit
//! -------------|--------------------|--------|---------------
//! `brightness` | Current brightness | Number | %
//!
//! # Example
//!
//! ```toml
//! [[block]]
//! block = "backlight"
//! device = "intel_backlight"
//! ```
//!
//! # Icons Used
//! - `backlight_empty` (when brightness between 0 and 6%)
//! - `backlight_1` (when brightness between 7 and 13%)
//! - `backlight_2` (when brightness between 14 and 20%)
//! - `backlight_3` (when brightness between 21 and 26%)
//! - `backlight_4` (when brightness between 27 and 33%)
//! - `backlight_5` (when brightness between 34 and 40%)
//! - `backlight_6` (when brightness between 41 and 46%)
//! - `backlight_7` (when brightness between 47 and 53%)
//! - `backlight_8` (when brightness between 54 and 60%)
//! - `backlight_9` (when brightness between 61 and 67%)
//! - `backlight_10` (when brightness between 68 and 73%)
//! - `backlight_11` (when brightness between 74 and 80%)
//! - `backlight_12` (when brightness between 81 and 87%)
//! - `backlight_13` (when brightness between 88 and 93%)
//! - `backlight_full` (when brightness above 94%)

use inotify::{Inotify, WatchMask};
use std::path::{Path, PathBuf};
use tokio::fs::read_dir;

use super::prelude::*;

/// Location of backlight devices
const DEVICES_PATH: &str = "/sys/class/backlight";

/// Filename for device's max brightness
const FILE_MAX_BRIGHTNESS: &str = "max_brightness";

/// Filename for current brightness.
const FILE_BRIGHTNESS: &str = "actual_brightness";

/// amdgpu drivers set the actual_brightness in a different scale than
/// [0, max_brightness], so we have to use the 'brightness' file instead.
/// This may be fixed in the new 5.7 kernel?
const FILE_BRIGHTNESS_AMD: &str = "brightness";

/// Ordered list of icons used to display lighting progress
const BACKLIGHT_ICONS: &[&str] = &[
    "backlight_empty",
    "backlight_1",
    "backlight_2",
    "backlight_3",
    "backlight_4",
    "backlight_5",
    "backlight_6",
    "backlight_7",
    "backlight_8",
    "backlight_9",
    "backlight_10",
    "backlight_11",
    "backlight_12",
    "backlight_13",
    "backlight_full",
];

pub struct Backlight {
    text: TextWidget,
    format: FormatTemplate,
    device: BacklightDevice,
    step_width: u8,
    minimum: u8,
    maximum: u8,
    cycle: Vec<u8>,
    cycle_index: usize,
    invert_icons: bool,
    on_click: Option<String>,
}

#[derive(Deserialize, Debug, SmartDefault)]
#[serde(deny_unknown_fields, default)]
pub struct BacklightConfig {
    device: Option<String>,
    format: FormatTemplate,
    #[default(5)]
    step_width: u8,
    #[default(5)]
    minimum: u8,
    #[default(100)]
    maximum: u8,
    cycle: Option<Vec<u8>>,
    #[default(1.0)]
    root_scaling: f64,
    invert_icons: bool,
}

#[async_trait]
impl ConfigBlock for Backlight {
    type Config = BacklightConfig;

    async fn new(
        id: usize,
        config: Self::Config,
        shared_config: SharedConfig,
        update_request: Sender<usize>,
    ) -> Result<Self> {
        let root_scaling = config.root_scaling.clamp(0.1, 10.0);
        let maximum = config.maximum.clamp(0, 100);
        let minimum = config.minimum.clamp(0, maximum);

        let device = match &config.device {
            None => BacklightDevice::default(root_scaling).await?,
            Some(path) => BacklightDevice::from_device(path, root_scaling).await?,
        };

        device.monitor(id, update_request)?;

        Ok(Self {
            text: TextWidget::new(id, 0, shared_config),
            format: config.format.with_default("{brightness}")?,
            device,
            step_width: config.step_width,
            minimum,
            maximum,
            cycle: config.cycle.unwrap_or_else(|| vec![minimum, maximum]),
            cycle_index: 0,
            invert_icons: false,
            on_click: None,
        })
    }

    fn override_on_click(&mut self) -> Option<&mut Option<String>> {
        Some(&mut self.on_click)
    }
}

#[async_trait]
impl Block for Backlight {
    async fn update(&mut self) -> Result<()> {
        let brightness = self.device.brightness().await?;

        let mut icon_index = (brightness as usize * BACKLIGHT_ICONS.len()) / 101;
        if self.invert_icons {
            icon_index = BACKLIGHT_ICONS.len() - icon_index - 1;
        }

        self.text.set_icon(BACKLIGHT_ICONS[icon_index])?;
        self.text.set_texts(self.format.render(&map! {
            "brightness" => Value::from_integer(brightness as i64).percents()
        })?);

        Ok(())
    }

    async fn click(&mut self, event: &I3BarEvent) -> Result<bool> {
        match event.button {
            MouseButton::Right => self.advance_cycle().await?,
            MouseButton::Left => {
                if let Some(ref cmd) = self.on_click {
                    spawn_child_async("sh", &["-c", cmd]).error_msg("could not spawn child")?
                } else {
                    self.advance_cycle().await?
                }
            }
            MouseButton::WheelUp => {
                self.device
                    .set_brightness(
                        (self.device.brightness().await? + self.step_width)
                            .clamp(self.minimum, self.maximum),
                    )
                    .await?;
            }
            MouseButton::WheelDown => {
                self.device
                    .set_brightness(
                        (self
                            .device
                            .brightness()
                            .await?
                            .saturating_sub(self.step_width))
                        .clamp(self.minimum, self.maximum),
                    )
                    .await?;
            }
            _ => (),
        }

        Ok(true)
    }

    fn view(&self) -> Vec<&dyn I3BarWidget> {
        vec![&self.text]
    }
}

impl Backlight {
    async fn advance_cycle(&mut self) -> Result<()> {
        if self.cycle.is_empty() {
            return Ok(());
        }
        let current = self.device.brightness().await?;
        let nearest = if self.cycle[self.cycle_index] == current {
            self.cycle_index // shortcut
        } else {
            let current = current as i64;
            // by default, restart cycle at nearest value
            let key = |idx: usize, val: i64| {
                // distance to current brightness is the first criterion
                let distance = (val - current).abs();
                // offset makes it so that in case of an equality for distance,
                // the winning index is the first one after cycle_index (circularly)
                let offset = if idx >= self.cycle_index {
                    0
                } else {
                    self.cycle.len()
                };
                (distance, idx + offset)
            };
            self.cycle
                .iter()
                .enumerate()
                .min_by_key(|&(idx, &val)| key(idx, val as i64))
                .unwrap() // cycle has been checked non-empty
                .0
        };
        self.cycle_index = (nearest + 1) % self.cycle.len();
        self.device
            .set_brightness(self.cycle[self.cycle_index])
            .await
    }
}

/// Read a brightness value from the given path.
async fn read_brightness_raw(device_file: &Path) -> Result<u64> {
    read_file(device_file)
        .await
        .error_msg("Failed to read brightness file")?
        .parse::<u64>()
        .error_msg("Failed to read value from brightness file")
}

/// Represents a physical backlight device whose brightness level can be queried.
struct BacklightDevice {
    device_name: String,
    brightness_file: PathBuf,
    max_brightness: u64,
    root_scaling: f64,
    dbus_proxy: SessionProxy<'static>,
}

impl BacklightDevice {
    async fn new(device_path: PathBuf, root_scaling: f64) -> Result<Self> {
        let dbus_conn = new_system_dbus_connection().await?;
        Ok(Self {
            brightness_file: device_path.join(if device_path.ends_with("amdgpu_bl0") {
                FILE_BRIGHTNESS_AMD
            } else {
                FILE_BRIGHTNESS
            }),
            device_name: device_path
                .file_name()
                .map(|x| x.to_str().unwrap().into())
                .error_msg("Malformed device path")?,
            max_brightness: read_brightness_raw(&device_path.join(FILE_MAX_BRIGHTNESS)).await?,
            root_scaling,
            dbus_proxy: SessionProxy::new(&dbus_conn)
                .await
                .error_msg("failed to create SessionProxy")?,
        })
    }

    /// Use the default backlit device, i.e. the first one found in the
    /// `/sys/class/backlight` directory.
    async fn default(root_scaling: f64) -> Result<Self> {
        let device = read_dir(DEVICES_PATH)
            .await
            .error_msg("Failed to read backlight device directory")?
            .next_entry()
            .await
            .error_msg("No backlit devices found")?
            .error_msg("Failed to read default device file")?;
        Self::new(device.path(), root_scaling).await
    }

    /// Use the backlit device `device`. Returns an error if a directory for
    /// that device is not found.
    async fn from_device(device: &str, root_scaling: f64) -> Result<Self> {
        Self::new(Path::new(DEVICES_PATH).join(device), root_scaling).await
    }

    /// Query the brightness value for this backlit device, as a percent.
    async fn brightness(&self) -> Result<u8> {
        let raw = read_brightness_raw(&self.brightness_file).await?;

        let brightness_ratio =
            (raw as f64 / self.max_brightness as f64).powf(self.root_scaling.recip());

        ((brightness_ratio * 100.0).round() as i64)
            .try_into()
            .ok()
            .filter(|brightness| (0..=100).contains(brightness))
            .error_msg("Brightness is not in [0, 100]")
    }

    /// Set the brightness value for this backlight device, as a percent.
    async fn set_brightness(&self, value: u8) -> Result<()> {
        let value = value.clamp(0, 100);
        let ratio = (value as f64 / 100.0).powf(self.root_scaling);
        let raw = ((ratio * (self.max_brightness as f64)).round() as u32).max(1);
        self.dbus_proxy
            .set_brightness("backlight", &self.device_name, raw)
            .await
            .error_msg("Failed to send D-Bus message")
    }

    fn monitor(&self, id: usize, update_request: Sender<usize>) -> Result<()> {
        let mut notify = Inotify::init().error_msg("Failed to start inotify")?;

        notify
            .add_watch(&self.brightness_file, WatchMask::MODIFY)
            .error_msg("Failed to watch brightness file")?;

        let mut file_changes = notify
            .event_stream([0; 1024])
            .error_msg("Failed to create event stream")?;

        tokio::spawn(async move {
            while file_changes.next().await.is_some() {
                if update_request.send(id).await.is_err() {
                    break;
                }
            }
        });

        Ok(())
    }
}

#[zbus::dbus_proxy(
    interface = "org.freedesktop.login1.Session",
    default_service = "org.freedesktop.login1",
    default_path = "/org/freedesktop/login1/session/auto"
)]
trait Session {
    fn set_brightness(&self, subsystem: &str, name: &str, brightness: u32) -> zbus::Result<()>;
}
