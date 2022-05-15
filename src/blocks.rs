mod base_block;
mod prelude;
use base_block::*;

use std::process::Command;
use std::time::Duration;

use serde::de::{self, Deserialize, DeserializeOwned};
use tokio::sync::mpsc::Sender;
use toml::value::Value;

use crate::config::SharedConfig;
use crate::errors::*;
use crate::protocol::i3bar_event::I3BarEvent;
use crate::widgets::I3BarWidget;

/// The ConfigBlock trait combines a constructor (new(...)) and an associated configuration type
/// to form a block that can be instantiated from a piece of TOML (from the block configuration).
/// The associated type has to be a deserializable struct, which you can then use to get your
/// configurations from. The template shows you how to instantiate a simple Text widget.
/// For more info on how to use widgets, just look into other Blocks. More documentation to come.
///
/// The sender object can be used to send asynchronous update request for any block from a separate
/// thread, provide you know the Block's ID. This advanced feature can be used to reduce
/// the number of system calls by asynchronously waiting for events. A usage example can be found
/// in the Music block, which updates only when dbus signals a new song.
#[async_trait::async_trait]
pub trait ConfigBlock: Block {
    type Config: DeserializeOwned;

    /// Creates a new block from the relevant configuration.
    async fn new(
        id: usize,
        config: Self::Config,
        shared_config: SharedConfig,
        update_request: Sender<usize>,
    ) -> Result<Self>
    where
        Self: Sized;

    /// TODO: write documentation
    fn override_on_click(&mut self) -> Option<&mut Option<String>> {
        None
    }
}

/// The Block trait is used to interact with a block after it has been instantiated from ConfigBlock.
#[async_trait::async_trait]
pub trait Block: Send + Sync {
    /// How frequently `.update()` should be called.
    ///
    /// Return `None` if you don't want your block to be polled (this might be usefull if your
    /// block is "async" and spawns a task internally to wait for events).
    fn interval(&self) -> Option<Duration> {
        None
    }

    /// Use this function to update the internal state of your block, for example during
    /// periodic updates.
    ///
    /// This functions is called in four cases:
    /// 1) After the block is created.
    /// 2) `.interval()` returned `Some(duration)` and at least `duration` passed from the last
    ///    call to `.update()`.
    /// 3) The blocks has send a "async" update request.
    /// 4) `.click()` returned `Ok(true)`.
    async fn update(&mut self) -> Result<()>;

    /// Here you can react to the user clicking your block.
    ///
    /// The I3BarEvent instance contains all fields to describe the click action, including mouse
    /// button. If block uses more that one widget, use the `event.instance` property to determine
    /// which widget was clicked.
    ///
    /// Return `Ok(true)` if you want `.update()` to be called.
    async fn click(&mut self, _event: &I3BarEvent) -> Result<bool> {
        Ok(false)
    }

    /// Use this function to return the widgets that comprise the UI of your component.
    ///
    /// The music block may, for example, be comprised of a text widget and multiple
    /// buttons (buttons are also TextWidgets). Use a vec to wrap the references to your view.
    fn view(&self) -> Vec<&dyn I3BarWidget>;
}

macro_rules! define_blocks {
    {
        $( $(#[cfg($attr: meta)])? $block: ident :: $block_type : ident $(,)? )*
    } => {
        $(
            $(#[cfg($attr)])?
            pub mod $block;
        )*

        #[derive(Debug, Clone, Copy)]
        pub enum BlockType {
            $(
                $(#[cfg($attr)])?
                #[allow(non_camel_case_types)]
                $block,
            )*
        }

        impl BlockType {
            pub async fn create_block(
                self,
                id: usize,
                block_config: Value,
                shared_config: SharedConfig,
                update_request: Sender<usize>,
            ) -> Result<Option<(Box<dyn Block>, BlockHandlers)>>
            {
                match self {
                    $(
                        $(#[cfg($attr)])?
                        Self::$block => {
                            create_block_typed::<$block::$block_type>(id, block_config, shared_config, update_request).await
                        }
                    )*
                }
            }

            pub fn name(
                self,
            ) -> &'static str
            {
                match self {
                    $(
                        $(#[cfg($attr)])?
                        Self::$block => {
                            stringify!($block)
                        }
                    )*
                }
            }
        }

        impl<'de> Deserialize<'de> for BlockType {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: de::Deserializer<'de>,
            {
                struct Visitor;

                impl<'de> de::Visitor<'de> for Visitor {
                    type Value = BlockType;

                    fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                        formatter.write_str("a block name")
                    }

                    fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
                    where
                        E: de::Error,
                    {
                        match v {
                            $(
                            $(#[cfg($attr)])?
                            stringify!($block) => Ok(BlockType::$block),
                            $(
                            #[cfg(not($attr))]
                            stringify!($block) => Err(E::custom(format!("Block '{}' has to be enabled at the compile time", stringify!($block)))),
                            )?
                            )*
                            unknown => Err(E::custom(format!("Unknown block '{unknown}'")))
                        }
                    }
                }

                deserializer.deserialize_str(Visitor)
            }
        }

    };
}

// Please keep these in alphabetical order.
define_blocks!(
    apt::Apt,
    backlight::Backlight,
    // battery::Battery,
    // bluetooth::Bluetooth,
    cpu::Cpu,
    // custom::Custom,
    // custom_dbus::CustomDBus,
    disk_space::DiskSpace,
    // dnf::Dnf,
    // docker::Docker,
    // external_ip::ExternalIP,
    // focused_window::FocusedWindow,
    github::Github,
    hueshift::Hueshift,
    // ibus::IBus,
    // kdeconnect::KDEConnect,
    // keyboard_layout::KeyboardLayout,
    load::Load,
    // #[cfg(feature = "maildir")]
    // maildir::Maildir,
    memory::Memory,
    // music::Music,
    // net::Net,
    // networkmanager::NetworkManager,
    // notify::Notify,
    // #[cfg(feature = "notmuch")]
    // notmuch::Notmuch,
    // nvidia_gpu::NvidiaGpu,
    // pacman::Pacman,
    // pomodoro::Pomodoro,
    // rofication::Rofication,
    sound::Sound,
    speedtest::SpeedTest,
    // taskwarrior::Taskwarrior,
    temperature::Temperature,
    time::Time,
    // toggle::Toggle,
    uptime::Uptime,
    // watson::Watson,
    // weather::Weather,
    // xrandr::Xrandr,
);

pub struct BlockHandlers {
    pub signal: Option<i32>,
    pub on_click: Option<String>,
}

pub async fn create_block_typed<B>(
    id: usize,
    mut block_config: Value,
    mut shared_config: SharedConfig,
    update_request: Sender<usize>,
) -> Result<Option<(Box<dyn Block>, BlockHandlers)>>
where
    B: ConfigBlock + 'static,
{
    // Extract base(common) config
    let common_config = BaseBlockConfig::extract(&mut block_config);
    let mut common_config = BaseBlockConfig::deserialize(common_config)
        .error_msg("Failed to deserialize common block config")?;

    // Run if_command if present
    if let Some(ref cmd) = common_config.if_command {
        if !Command::new("sh")
            .args(["-c", cmd])
            .output()
            .map_or(false, |o| o.status.success())
        {
            return Ok(None);
        }
    }

    // Apply theme overrides if presented
    if let Some(ref overrides) = common_config.theme_overrides {
        shared_config.theme_override(overrides)?;
    }
    if let Some(overrides) = common_config.icons_format {
        shared_config.icons_format_override(overrides);
    }
    if let Some(overrides) = common_config.icons_overrides {
        shared_config.icons_override(overrides);
    }

    // Extract block-specific config
    let block_config =
        B::Config::deserialize(block_config).error_msg("Failed to deserialize block config")?;

    let mut block = B::new(id, block_config, shared_config, update_request).await?;
    if let Some(overrided) = block.override_on_click() {
        *overrided = common_config.on_click.take();
    }

    Ok(Some((
        Box::new(block),
        BlockHandlers {
            signal: common_config.signal,
            on_click: common_config.on_click,
        },
    )))
}
