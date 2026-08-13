use std::collections::HashMap;
use std::time::Duration;

use zbus::MessageStream;

use futures_lite::stream::StreamExt;

use crate::config::Config;
use crate::device::{build_devices, Device};
use crate::udisks::{is_relevant_signal, UdisksClient};

const DEBOUNCE_MS: u64 = 150;

pub struct Monitor<'a> {
    config: &'a Config,
    client: UdisksClient,
    mount_time: HashMap<String, i64>,
    insertion_seq: HashMap<String, u64>,
    next_insertion: u64,
}

impl<'a> Monitor<'a> {
    pub fn new(config: &'a Config) -> Self {
        Monitor {
            config,
            client: UdisksClient::new(),
            mount_time: HashMap::new(),
            insertion_seq: HashMap::new(),
            next_insertion: 0,
        }
    }

    // Connect, subscribe and loop until SIGINT/SIGTERM, emitting on every
    // debounced change.
    pub async fn run<F>(&mut self, mut emit: F) -> Result<(), String>
    where
        F: FnMut(&[Device]),
    {
        if !self.client.connect().await {
            return Err(format!("cannot connect to system bus: {}", self.client.last_error()));
        }
        let connection = self
            .client
            .connection()
            .ok_or_else(|| "system bus connection lost".to_string())?;
        let mut stream = MessageStream::from(&connection);

        use tokio::signal::unix::{signal, SignalKind};
        let mut sigint = signal(SignalKind::interrupt()).map_err(|e| e.to_string())?;
        let mut sigterm = signal(SignalKind::terminate()).map_err(|e| e.to_string())?;

        self.refresh(&mut emit).await;
        let mut pending = false;
        loop {
            if pending {
                tokio::select! {
                    _ = tokio::time::sleep(Duration::from_millis(DEBOUNCE_MS)) => {
                        self.refresh(&mut emit).await;
                        pending = false;
                    }
                    _ = sigint.recv() => return Ok(()),
                    _ = sigterm.recv() => return Ok(()),
                    msg = stream.next() => {
                        if msg.is_none() {
                            return Ok(());
                        }
                    }
                }
            } else {
                tokio::select! {
                    _ = sigint.recv() => return Ok(()),
                    _ = sigterm.recv() => return Ok(()),
                    msg = stream.next() => {
                        match msg {
                            Some(Ok(m)) if is_relevant_signal(&m) => pending = true,
                            Some(Ok(_)) => {}
                            Some(Err(_)) => {}
                            None => return Ok(()),
                        }
                    }
                }
            }
        }
    }

    async fn refresh<F>(&mut self, emit: &mut F)
    where
        F: FnMut(&[Device]),
    {
        let raw = self.client.enumerate().await;
        let devices = build_devices(
            &raw,
            self.config,
            &mut self.mount_time,
            &mut self.insertion_seq,
            &mut self.next_insertion,
        );
        emit(&devices);
    }
}
