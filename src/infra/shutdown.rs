use tokio::sync::watch;

#[derive(Clone)]
pub struct ShutdownSignal {
    sender: watch::Sender<bool>,
}

impl ShutdownSignal {
    pub fn new() -> Self {
        let (sender, _) = watch::channel(false);
        Self { sender }
    }

    pub fn subscribe(&self) -> watch::Receiver<bool> {
        self.sender.subscribe()
    }

    pub fn trigger(&self) {
        let _ = self.sender.send(true);
    }
}
