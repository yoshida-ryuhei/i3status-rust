use std::str::FromStr;
use tokio::fs::File;
use tokio::io::{AsyncBufReadExt, BufReader};

use super::prelude::*;
use crate::util::read_file;

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "lowercase")]
pub enum Memtype {
    Swap,
    Memory,
}

impl Memtype {
    fn icon(self) -> &'static str {
        match self {
            Self::Swap => "memory_swap",
            Self::Memory => "memory_mem",
        }
    }
}

#[derive(Clone, Debug)]
pub struct Memory {
    memtype: Memtype,
    text: TextWidget,
    icons: bool,
    clickable: bool,
    format: (FormatTemplate, FormatTemplate),
    update_interval: Duration,
    warning: (f64, f64),
    critical: (f64, f64),
}

#[derive(Deserialize, Debug, Clone, SmartDefault)]
#[serde(deny_unknown_fields, default)]
pub struct MemoryConfig {
    format_mem: FormatTemplate,
    format_swap: FormatTemplate,
    #[default(Memtype::Memory)]
    display_type: Memtype,
    // Deprecated
    #[default(true)]
    icons: bool,
    #[default(true)]
    clickable: bool,
    #[default(5.into())]
    interval: Seconds,
    #[default(80.0)]
    warning_mem: f64,
    #[default(80.0)]
    warning_swap: f64,
    #[default(95.0)]
    critical_mem: f64,
    #[default(95.0)]
    critical_swap: f64,
}

#[async_trait]
impl ConfigBlock for Memory {
    type Config = MemoryConfig;

    async fn new(
        id: usize,
        config: Self::Config,
        shared_config: SharedConfig,
        _: Sender<usize>,
    ) -> Result<Self> {
        Ok(Memory {
            memtype: config.display_type,
            text: TextWidget::new(id, 0, shared_config).with_icon(config.display_type.icon())?,
            icons: config.icons,
            clickable: config.clickable,
            format: (
                config
                    .format_mem
                    .with_default("{mem_free;M}/{mem_total;M}({mem_total_used_percents})")?,
                config
                    .format_swap
                    .with_default("{swap_free;M}/{swap_total;M}({swap_used_percents})")?,
            ),
            update_interval: config.interval.0,
            warning: (config.warning_mem, config.warning_swap),
            critical: (config.critical_mem, config.critical_swap),
        })
    }
}

#[async_trait]
impl Block for Memory {
    fn interval(&self) -> Option<Duration> {
        Some(self.update_interval)
    }

    async fn update(&mut self) -> Result<()> {
        let mem_state = Memstate::new().await?;
        let mem_total = mem_state.mem_total as f64 * 1024.;
        let mem_free = mem_state.mem_free as f64 * 1024.;
        let swap_total = mem_state.swap_total as f64 * 1024.;
        let swap_free = mem_state.swap_free as f64 * 1024.;
        let swap_used = swap_total - swap_free;
        let mem_total_used = mem_total - mem_free;
        let buffers = mem_state.buffers as f64 * 1024.;
        let cached = (mem_state.cached + mem_state.s_reclaimable - mem_state.shmem) as f64 * 1024.
            + mem_state.zfs_arc_cache as f64;
        let mem_used = mem_total_used - (buffers + cached);
        let mem_avail = mem_total - mem_used;

        let values = map! {
            "mem_total" => Value::from_float(mem_total).bytes(),
            "mem_free" => Value::from_float(mem_free).bytes(),
            "mem_free_percents" => Value::from_float(mem_free / mem_total * 100.).percents(),
            "mem_total_used" => Value::from_float(mem_total_used).bytes(),
            "mem_total_used_percents" => Value::from_float(mem_total_used / mem_total * 100.).percents(),
            "mem_used" => Value::from_float(mem_used).bytes(),
            "mem_used_percents" => Value::from_float(mem_used / mem_total * 100.).percents(),
            "mem_avail" => Value::from_float(mem_avail).bytes(),
            "mem_avail_percents" => Value::from_float(mem_avail / mem_total * 100.).percents(),
            "swap_total" => Value::from_float(swap_total).bytes(),
            "swap_free" => Value::from_float(swap_free).bytes(),
            "swap_free_percents" => Value::from_float(swap_free / swap_total * 100.).percents(),
            "swap_used" => Value::from_float(swap_used).bytes(),
            "swap_used_percents" => Value::from_float(swap_used / swap_total * 100.).percents(),
            "buffers" => Value::from_float(buffers).bytes(),
            "buffers_percent" => Value::from_float(buffers / mem_total * 100.).percents(),
            "cached" => Value::from_float(cached).bytes(),
            "cached_percent" => Value::from_float(cached / mem_total * 100.).percents(),
        };

        self.text.set_state(match self.memtype {
            Memtype::Memory => match mem_used / mem_total * 100. {
                x if x > self.critical.0 => State::Critical,
                x if x > self.warning.0 => State::Warning,
                _ => State::Idle,
            },
            Memtype::Swap => match swap_used / swap_total * 100. {
                x if x > self.critical.1 => State::Critical,
                x if x > self.warning.1 => State::Warning,
                _ => State::Idle,
            },
        });

        if self.icons {
            self.text.set_icon(match self.memtype {
                Memtype::Swap => "memory_swap",
                Memtype::Memory => "memory_mem",
            })?;
        }

        self.text.set_texts(match self.memtype {
            Memtype::Memory => self.format.0.render(&values)?,
            Memtype::Swap => self.format.1.render(&values)?,
        });
        Ok(())
    }

    async fn click(&mut self, event: &I3BarEvent) -> Result<bool> {
        if event.button == MouseButton::Left && self.clickable {
            self.memtype = match self.memtype {
                Memtype::Memory => Memtype::Swap,
                Memtype::Swap => Memtype::Memory,
            };
            self.text.set_icon(self.memtype.icon())?;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    fn view(&self) -> Vec<&dyn I3BarWidget> {
        vec![&self.text]
    }
}

#[derive(Clone, Copy, Debug, Default)]
struct Memstate {
    mem_total: u64,
    mem_free: u64,
    buffers: u64,
    cached: u64,
    s_reclaimable: u64,
    shmem: u64,
    swap_total: u64,
    swap_free: u64,
    zfs_arc_cache: u64,
}

impl Memstate {
    async fn new() -> Result<Self> {
        let mut file = BufReader::new(
            File::open("/proc/meminfo")
                .await
                .error_msg("/proc/meminfo does not exist")?,
        );

        let mut mem_state = Memstate::default();
        let mut line = String::new();

        while file
            .read_line(&mut line)
            .await
            .error_msg("failed to read /proc/meminfo")?
            != 0
        {
            let mut words = line.split_whitespace();

            let name = match words.next() {
                Some(name) => name,
                None => {
                    line.clear();
                    continue;
                }
            };
            let val = words
                .next()
                .and_then(|x| u64::from_str(x).ok())
                .error_msg("failed to parse /proc/meminfo")?;

            match name {
                "MemTotal:" => mem_state.mem_total = val,
                "MemFree:" => mem_state.mem_free = val,
                "Buffers:" => mem_state.buffers = val,
                "Cached:" => mem_state.cached = val,
                "SReclaimable:" => mem_state.s_reclaimable = val,
                "Shmem:" => mem_state.shmem = val,
                "SwapTotal:" => mem_state.swap_total = val,
                "SwapFree:" => mem_state.swap_free = val,
                _ => (),
            }

            line.clear();
        }

        // Read ZFS arc cache size to add to total cache size
        if let Ok(arcstats) = read_file("/proc/spl/kstat/zfs/arcstats").await {
            let size_re = regex!(r"size\s+\d+\s+(\d+)");
            let size = &size_re
                .captures(&arcstats)
                .error_msg("failed to find zfs_arc_cache size")?[1];
            mem_state.zfs_arc_cache = size
                .parse()
                .error_msg("failed to parse zfs_arc_cache size")?;
        }

        Ok(mem_state)
    }
}
