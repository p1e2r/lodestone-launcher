//! Shared, thread-safe state for long-running jobs (installs, running games,
//! running servers). Background tasks write into a `Session`; the UI reads a
//! snapshot every frame and re-renders when `EPOCH` changes.

use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, LazyLock, Mutex};

use tokio::sync::{Notify, mpsc::UnboundedSender};

pub static EPOCH: AtomicU64 = AtomicU64::new(0);

pub fn bump() {
    EPOCH.fetch_add(1, Ordering::Relaxed);
}

pub fn epoch() -> u64 {
    EPOCH.load(Ordering::Relaxed)
}

const MAX_LOG: usize = 6000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Level {
    Info,
    Warn,
    Error,
    Launcher,
}

#[derive(Clone, Debug)]
pub struct LogLine {
    pub level: Level,
    pub text: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RunState {
    Idle,
    Working,
    Running,
    Failed(String),
}

pub struct Session {
    pub state: RunState,
    pub stage: String,
    pub done: u64,
    pub total: u64,
    pub log: VecDeque<LogLine>,
    pub stdin: Option<UnboundedSender<String>>,
    pub kill: Option<Arc<Notify>>,
    pub started_at: Option<std::time::Instant>,
}

impl Default for Session {
    fn default() -> Self {
        Self {
            state: RunState::Idle,
            stage: String::new(),
            done: 0,
            total: 0,
            log: VecDeque::new(),
            stdin: None,
            kill: None,
            started_at: None,
        }
    }
}

impl Session {
    pub fn busy(&self) -> bool {
        matches!(self.state, RunState::Working | RunState::Running)
    }
    pub fn fraction(&self) -> f32 {
        if self.total == 0 {
            0.0
        } else {
            (self.done as f32 / self.total as f32).clamp(0.0, 1.0)
        }
    }
}

static SESSIONS: LazyLock<Mutex<HashMap<String, Arc<Mutex<Session>>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

pub fn session(key: &str) -> Arc<Mutex<Session>> {
    SESSIONS
        .lock()
        .unwrap()
        .entry(key.to_string())
        .or_default()
        .clone()
}

pub fn any_busy() -> bool {
    SESSIONS
        .lock()
        .unwrap()
        .values()
        .any(|s| s.lock().map(|s| s.busy()).unwrap_or(false))
}

/// Handle given to background work to report progress and log lines.
#[derive(Clone)]
pub struct Reporter {
    pub session: Arc<Mutex<Session>>,
    /// Also print to stdout (used by the command-line mode).
    pub echo: bool,
}

impl Reporter {
    pub fn new(key: &str) -> Self {
        Self {
            session: session(key),
            echo: false,
        }
    }

    pub fn stage(&self, stage: impl Into<String>) {
        let stage = stage.into();
        if self.echo {
            println!("==> {stage}");
        }
        {
            let mut s = self.session.lock().unwrap();
            s.stage = stage.clone();
            s.done = 0;
            s.total = 0;
            push(&mut s, Level::Launcher, stage);
        }
        bump();
    }

    pub fn progress(&self, done: u64, total: u64) {
        let mut s = self.session.lock().unwrap();
        s.done = done;
        s.total = total;
        drop(s);
        bump();
    }

    pub fn inc(&self) {
        let mut s = self.session.lock().unwrap();
        s.done += 1;
        drop(s);
        bump();
    }

    pub fn log(&self, level: Level, text: impl Into<String>) {
        let text = text.into();
        if self.echo {
            println!("{text}");
        }
        let mut s = self.session.lock().unwrap();
        push(&mut s, level, text);
        drop(s);
        bump();
    }

    pub fn info(&self, text: impl Into<String>) {
        self.log(Level::Launcher, text)
    }

    pub fn warn(&self, text: impl Into<String>) {
        self.log(Level::Warn, text)
    }

    pub fn set_state(&self, state: RunState) {
        let mut s = self.session.lock().unwrap();
        if let RunState::Failed(e) = &state {
            push(&mut s, Level::Error, format!("Error: {e}"));
        }
        if state == RunState::Running {
            s.started_at = Some(std::time::Instant::now());
        }
        if !matches!(state, RunState::Running) {
            s.stdin = None;
            s.kill = None;
        }
        s.state = state;
        drop(s);
        bump();
    }

    pub fn clear_log(&self) {
        self.session.lock().unwrap().log.clear();
        bump();
    }
}

fn push(s: &mut Session, level: Level, text: String) {
    if s.log.len() >= MAX_LOG {
        s.log.pop_front();
    }
    s.log.push_back(LogLine { level, text });
}
