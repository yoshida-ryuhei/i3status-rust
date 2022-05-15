#[macro_use]
mod util;
mod apcaccess;
mod blocks;
mod config;
mod errors;
mod formatting;
mod icons;
mod protocol;
mod scheduler;
mod signals;
mod subprocess;
mod themes;
mod widgets;
mod wrappers;

use config::Scrolling;
#[cfg(feature = "pulseaudio")]
use libpulse_binding as pulse;

use std::future::Future;
use std::pin::Pin;
use std::time::{Duration, Instant};

use clap::Parser;
use futures::future::FutureExt;
use futures::stream::{FuturesUnordered, Stream, StreamExt};
use once_cell::sync::Lazy;
use tokio::sync::mpsc;

use crate::blocks::{Block, BlockHandlers};
use crate::config::{Config, SharedConfig};
use crate::errors::*;
use crate::protocol::i3bar_event::{events_stream, MouseButton};
use crate::scheduler::UpdateScheduler;
use crate::signals::{signals_stream, Signal};
use crate::subprocess::spawn_child_async;
use crate::util::deserialize_file;
use crate::widgets::text::TextWidget;
use crate::widgets::{I3BarWidget, State};

pub type BoxedFuture<T> = Pin<Box<dyn Future<Output = T>>>;
pub type BoxedStream<T> = Pin<Box<dyn Stream<Item = T>>>;

pub static REQWEST_CLIENT: Lazy<reqwest::Client> = Lazy::new(|| {
    const APP_USER_AGENT: &str = concat!(env!("CARGO_PKG_NAME"), "/", env!("CARGO_PKG_VERSION"),);
    reqwest::Client::builder()
        .user_agent(APP_USER_AGENT)
        .build()
        .unwrap()
});

#[derive(Debug, Parser)]
#[clap(author, about, version = env!("VERSION"))]
struct CliArgs {
    /// Sets a TOML config file
    config: Option<String>,
    /// Ignore any attempts by i3 to pause the bar when hidden/fullscreen
    #[clap(long = "never-pause")]
    never_pause: bool,
    /// Do not send the init sequence
    #[clap(long = "no-init")]
    no_init: bool,
    /// The maximum number of blocking threads spawned by tokio
    #[clap(long = "threads", short = 'j', default_value = "2")]
    blocking_threads: usize,
    /// The DBUS name
    #[clap(long = "dbus-name", default_value = "rs.i3status")]
    dbus_name: String,
}

#[tokio::main]
async fn main() {
    let args = CliArgs::parse();

    // Run and match for potential error
    if let Err(error) = run(args).await {
        // Create widget with error message
        let error_widget = TextWidget::new(0, 0, Default::default())
            .with_state(State::Critical)
            .with_text(&format!("Error: {error}"));

        // Print errors
        println!("[{}],", error_widget.get_data().render());
        eprintln!("\n\n{:?}", error);

        // Wait for USR2 signal to restart
        signal_hook::iterator::Signals::new(&[signal_hook::consts::SIGUSR2])
            .unwrap()
            .forever()
            .next()
            .unwrap();
        restart();
    }
}

async fn run(args: CliArgs) -> Result<()> {
    if !args.no_init {
        // Now we can start to run the i3bar protocol
        protocol::init(args.never_pause);
    }

    // Read & parse the config file
    let config_path = match args.config {
        Some(config_path) => std::path::PathBuf::from(config_path),
        None => util::xdg_config_home().join("i3status-rust/config.toml"),
    };
    let config: Config = deserialize_file(&config_path)?;

    let shared_config = SharedConfig::new(&config);
    let (tx_update_requests, mut rx_update_requests) = mpsc::channel(32);

    struct BlockState {
        inner: Option<Box<dyn Block>>,
        handlers: BlockHandlers,
    }

    // Initialize the blocks
    let mut blocks = Vec::<Option<BlockState>>::new();
    let mut blocks_names = Vec::new();
    let mut block_rendered = Vec::new();
    let mut pending_new_blocks = FuturesUnordered::new();
    let mut pending_updating_blocks = FuturesUnordered::new();
    for (block_name, block_config) in config.blocks {
        let id = blocks.len();
        let block = block_name
            .create_block(
                id,
                block_config,
                shared_config.clone(),
                tx_update_requests.clone(),
            )
            .map(move |res| (id, res));

        blocks.push(None);
        blocks_names.push(block_name.name());
        pending_new_blocks.push(block);
        block_rendered.push(Vec::new());
    }

    let mut scheduler = UpdateScheduler::new(blocks.len());
    let mut signals_stream = signals_stream();
    let mut events_stream = events_stream(config.scrolling == Scrolling::Natural, Duration::ZERO);

    // Time to next update channel.
    // Fires immediately for first updates
    let mut ttnu = Duration::from_secs(100);

    fn update_block(
        id: usize,
        mut block: Box<dyn Block>,
    ) -> impl Future<Output = (usize, Box<dyn Block>, Result<()>)> {
        async move {
            let res = block.update().await;
            (id, block, res)
        }
    }

    loop {
        // We use the message passing concept of channel selection
        // to avoid busy wait
        tokio::select! {
            // Created blocks
            Some((id, block)) = pending_new_blocks.next() => {
                if let Some((block, handlers)) = block.in_block(blocks_names[id])? {
                    blocks[id] = Some(BlockState { inner: None, handlers });
                    block_rendered[id] = block.view().iter().map(|w| w.get_data()).collect();
                    pending_updating_blocks.push(update_block(id, block));
                    protocol::print_blocks(&block_rendered, &shared_config)?;
                }
            }
            // Updated blocks
            Some((id, block, result)) = pending_updating_blocks.next() => {
                result.in_block(blocks_names[id])?;
                block_rendered[id] = block.view().iter().map(|w| w.get_data()).collect();
                scheduler.pop(id);
                if let Some(dur) = block.interval() {
                    scheduler.push(id, Instant::now() + dur);
                }
                blocks[id].as_mut().unwrap().inner = Some(block);
                protocol::print_blocks(&block_rendered, &shared_config)?;
            }
            // Receive async update requests
            Some(id) = rx_update_requests.recv() => {
                if let Some(block) = &mut blocks[id] {
                    if let Some(block) = block.inner.take() {
                        pending_updating_blocks.push(update_block(id, block));
                    }
                }
            },
            // Receive click events
            Some(event) = events_stream.next() => {
                let id = event.id;
                if let Some(block) = &mut blocks[id] {
                    match &block.handlers.on_click {
                        Some(on_click) if event.button == MouseButton::Left => {
                            spawn_child_async("sh", &["-c", on_click])
                                .error_msg("could not spawn child")
                                .in_block(blocks_names[id])?;
                        }
                        _ => {
                            // TODO: keep track of pending click events
                            if let Some(mut block) = block.inner.take(){
                                // TODO: this remove this .await
                                if block.click(&event).await.in_block(blocks_names[id])? {
                                    pending_updating_blocks.push(update_block(event.id, block));
                                }
                            }
                        }
                    }
                }
            },
            // Receive update timer events
            _ = tokio::time::sleep(ttnu) => {
                let now = Instant::now();
                scheduler.schedule.retain(|task| {
                    if task.update_time <= now {
                        let id = task.id;
                        if let Some(block) = &mut blocks[id] {
                            if let Some(block) = block.inner.take() {
                                pending_updating_blocks.push(update_block(id, block));
                            }
                        }
                        false
                    } else {
                        true
                    }
                });
            },
            // Receive signal events
            Some(sig) = signals_stream.next() => {
                match sig {
                    Signal::Usr1 => {
                        //USR1 signal that updates every block in the bar
                        for (id, block) in blocks.iter_mut().filter_map(|b| b.as_mut()).enumerate() {
                            if let Some(block) = block.inner.take() {
                                pending_updating_blocks.push(update_block(id, block));
                            }
                        }
                    },
                    Signal::Usr2 => {
                        //USR2 signal that should reload the config
                        restart();
                    },
                    Signal::Other(sig) => {
                        //Real time signal that updates only the blocks listening
                        //for that signal
                        for (id, block) in blocks.iter_mut().filter_map(|b| b.as_mut()).enumerate() {
                            if block.handlers.signal == Some(sig) {
                                if let Some(block) = block.inner.take() {
                                    pending_updating_blocks.push(update_block(id, block));
                                }
                            }
                        }
                    },
                };
            }
        }

        // Set the time-to-next-update timer
        if let Some(time) = scheduler.time_to_next_update() {
            ttnu = time;
        }
    }
}

/// Restart `i3status-rs` in-place
fn restart() -> ! {
    use std::env;
    use std::ffi::CString;
    use std::os::unix::ffi::OsStringExt;

    // On linux this line should be OK
    let exe = CString::new(env::current_exe().unwrap().into_os_string().into_vec()).unwrap();

    // Get current arguments
    let mut arg = env::args()
        .map(|a| CString::new(a).unwrap())
        .collect::<Vec<CString>>();

    // Add "--no-init" argument if not already added
    let no_init_arg = CString::new("--no-init").unwrap();
    if !arg.iter().any(|a| *a == no_init_arg) {
        arg.push(no_init_arg);
    }

    // Restart
    nix::unistd::execvp(&exe, &arg).unwrap();
    unreachable!();
}
