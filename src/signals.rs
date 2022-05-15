use crate::BoxedStream;
use futures::stream::StreamExt;
use libc::{SIGRTMAX, SIGRTMIN};
use signal_hook::consts::{SIGUSR1, SIGUSR2};
use signal_hook_tokio::Signals;

pub enum Signal {
    Usr1,
    Usr2,
    Other(i32),
}

/// Returns an infinite stream of `Signal`s
pub fn signals_stream() -> BoxedStream<Signal> {
    let (sigmin, sigmax) = (SIGRTMIN(), SIGRTMAX());
    let signals = Signals::new((sigmin..sigmax).chain([SIGUSR1, SIGUSR2])).unwrap();
    signals
        .map(move |signal| match signal {
            SIGUSR1 => Signal::Usr1,
            SIGUSR2 => Signal::Usr2,
            x => Signal::Other(x - sigmin),
        })
        .boxed()
}
