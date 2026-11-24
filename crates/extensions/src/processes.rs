use std::collections::HashMap;

use tokio::process::Child;
use tokio::sync::{mpsc, oneshot};

use crate::process_tree::ProcessTree;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcessStatus {
    Running,
    Stopped,
    Exited { code: Option<i32> },
}

#[derive(Debug)]
pub struct Exit {
    id: String,
    launch: u64,
    code: Option<i32>,
}

struct Watched {
    launch: u64,
    stop: oneshot::Sender<oneshot::Sender<()>>,
}

pub struct Processes {
    running: HashMap<String, Watched>,
    exited: HashMap<String, Option<i32>>,
    exits: mpsc::UnboundedSender<Exit>,
    launches: u64,
}

impl Processes {
    pub fn new() -> (Self, mpsc::UnboundedReceiver<Exit>) {
        let (exits, receiver) = mpsc::unbounded_channel();
        let processes = Self {
            running: HashMap::new(),
            exited: HashMap::new(),
            exits,
            launches: 0,
        };
        (processes, receiver)
    }

    pub fn watch(&mut self, id: &str, mut child: Child) {
        self.launches += 1;
        let launch = self.launches;
        let (stop, stopped) = oneshot::channel::<oneshot::Sender<()>>();
        let exits = self.exits.clone();
        let watched = id.to_owned();
        let tree = ProcessTree::adopt(&child).ok();
        tokio::spawn(async move {
            tokio::select! {
                status = child.wait() => {
                    let code = status.ok().and_then(|status| status.code());
                    let _ = exits.send(Exit { id: watched, launch, code });
                }
                done = stopped => {
                    if let Some(tree) = tree {
                        tree.end();
                    }
                    let _ = child.kill().await;
                    if let Ok(done) = done {
                        let _ = done.send(());
                    }
                }
            }
        });
        self.exited.remove(id);
        self.running.insert(id.to_owned(), Watched { launch, stop });
    }

    pub async fn stop(&mut self, id: &str) {
        self.exited.remove(id);
        if let Some(watched) = self.running.remove(id) {
            let (done, finished) = oneshot::channel();
            if watched.stop.send(done).is_ok() {
                let _ = finished.await;
            }
        }
    }

    pub fn record(&mut self, exit: Exit) -> bool {
        let current = self
            .running
            .get(&exit.id)
            .is_some_and(|watched| watched.launch == exit.launch);
        if current {
            self.running.remove(&exit.id);
            self.exited.insert(exit.id, exit.code);
        }
        current
    }

    pub fn status(&self, id: &str) -> ProcessStatus {
        match (self.running.contains_key(id), self.exited.get(id)) {
            (true, _) => ProcessStatus::Running,
            (false, Some(&code)) => ProcessStatus::Exited { code },
            (false, None) => ProcessStatus::Stopped,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use tokio::process::Command;

    use super::*;

    const LINGER: &str = "BOBBA_TEST_LINGER";

    #[test]
    fn lingers_when_asked() {
        if std::env::var_os(LINGER).is_some() {
            std::thread::sleep(Duration::from_secs(30));
        }
    }

    fn this_test_binary() -> Command {
        let mut command = Command::new(std::env::current_exe().expect("test binary"));
        command.kill_on_drop(true);
        command
    }

    fn quits_with_an_error() -> Child {
        this_test_binary()
            .arg("--no-such-flag")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .expect("spawns")
    }

    fn lingers() -> Child {
        this_test_binary()
            .args(["--exact", "processes::tests::lingers_when_asked"])
            .env(LINGER, "1")
            .stdout(std::process::Stdio::null())
            .spawn()
            .expect("spawns")
    }

    #[tokio::test]
    async fn a_process_that_quits_on_its_own_is_reported_with_its_code() {
        let (mut processes, mut exits) = Processes::new();
        processes.watch("presets", quits_with_an_error());
        assert_eq!(processes.status("presets"), ProcessStatus::Running);

        let exit = tokio::time::timeout(Duration::from_secs(10), exits.recv())
            .await
            .expect("exits in time")
            .expect("reports the exit");
        assert!(processes.record(exit));
        assert!(matches!(
            processes.status("presets"),
            ProcessStatus::Exited { code: Some(code) } if code != 0
        ));
    }

    #[tokio::test]
    async fn a_stopped_process_is_stopped_not_exited() {
        let (mut processes, mut exits) = Processes::new();
        processes.watch("presets", lingers());
        processes.stop("presets").await;
        assert_eq!(processes.status("presets"), ProcessStatus::Stopped);
        let reported = tokio::time::timeout(Duration::from_millis(300), exits.recv()).await;
        assert!(reported.is_err(), "no exit is reported for a stop");
    }

    #[tokio::test]
    async fn an_exit_from_an_earlier_launch_leaves_the_new_one_running() {
        let (mut processes, mut exits) = Processes::new();
        processes.watch("presets", quits_with_an_error());
        let stale = tokio::time::timeout(Duration::from_secs(10), exits.recv())
            .await
            .expect("exits in time")
            .expect("reports the exit");
        processes.stop("presets").await;
        processes.watch("presets", lingers());

        assert!(!processes.record(stale));
        assert_eq!(processes.status("presets"), ProcessStatus::Running);
        processes.stop("presets").await;
    }
}
