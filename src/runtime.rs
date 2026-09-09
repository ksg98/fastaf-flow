use anyhow::{Context, Result, bail};
use serde_json::{Value, json};
use std::{
    fs,
    io::{BufRead, BufReader, Write},
    path::PathBuf,
    process::{Child, ChildStdin, ChildStdout, Command, Stdio},
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
        mpsc,
    },
    thread,
};

pub enum Event {
    Progress(u64, String),
    Done(u64, Value),
    Error(u64, String),
}
struct Request {
    id: u64,
    body: Value,
}
pub struct Service {
    tx: mpsc::Sender<Request>,
    pub rx: mpsc::Receiver<Event>,
    child: Arc<Mutex<Option<Child>>>,
    active: Arc<AtomicU64>,
}
fn worker_dir() -> PathBuf {
    if let Ok(exe) = std::env::current_exe()
        && let Some(contents) = exe.parent().and_then(|p| p.parent())
    {
        let resources = contents.join("Resources/worker");
        if resources.join("engine.py").exists() {
            return resources;
        }
    }
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("worker")
}
pub fn python() -> PathBuf {
    if let Some(path) = std::env::var_os("FASTAF_PYTHON") {
        return path.into();
    }
    let installed = crate::settings::data_dir().join("runtime/.venv/bin/python");
    if installed.exists() {
        return installed;
    }
    worker_dir().join(".venv/bin/python")
}
pub fn installed() -> bool {
    python().exists()
}
fn uv() -> Result<PathBuf> {
    let mut candidates = vec![
        worker_dir().join("uv"),
        PathBuf::from("/opt/homebrew/bin/uv"),
        PathBuf::from("/usr/local/bin/uv"),
    ];
    if let Some(home) = directories::BaseDirs::new() {
        candidates.push(home.home_dir().join(".local/bin/uv"));
    }
    if let Some(paths) = std::env::var_os("PATH") {
        candidates.extend(std::env::split_paths(&paths).map(|p| p.join("uv")));
    }
    candidates.into_iter().find(|p| p.is_file()).context("Install uv from https://docs.astral.sh/uv/getting-started/installation/ and choose Install runtime again.")
}
fn setup(
    child: &Arc<Mutex<Option<Child>>>,
    active: &AtomicU64,
    id: u64,
    report: &impl Fn(String),
) -> Result<Value> {
    if !cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        bail!("MLX requires an Apple Silicon Mac.");
    }
    let dir = crate::settings::data_dir().join("runtime");
    fs::create_dir_all(&dir)?;
    for name in ["pyproject.toml", "uv.lock"] {
        fs::copy(worker_dir().join(name), dir.join(name))?;
    }
    report(
        "Installing the local MLX runtime. First setup downloads Python and dependencies…".into(),
    );
    let mut cmd = Command::new(uv()?);
    cmd.args([
        "sync",
        "--frozen",
        "--no-dev",
        "--managed-python",
        "--python",
        "3.12",
        "--project",
    ])
    .arg(&dir)
    .env(
        "UV_PYTHON_INSTALL_DIR",
        crate::settings::data_dir().join("python"),
    )
    .env_remove("VIRTUAL_ENV")
    .stdin(Stdio::null())
    .stdout(Stdio::null())
    .stderr(Stdio::piped());
    let mut process = cmd
        .spawn()
        .context("Could not start the runtime installer")?;
    let stderr = process.stderr.take().unwrap();
    *child.lock().unwrap() = Some(process);
    if active.load(Ordering::SeqCst) != id {
        stop(child);
        bail!("Installation cancelled");
    }
    let mut tail = String::new();
    for line in BufReader::new(stderr).lines() {
        let line = line?;
        if !line.trim().is_empty() {
            report(line.clone());
            tail = line;
        }
    }
    let mut process = child
        .lock()
        .unwrap()
        .take()
        .context("Installation cancelled")?;
    if !process.wait()?.success() {
        bail!("Runtime installation stopped: {tail}");
    }
    Ok(json!({"installed": true}))
}
struct Process {
    input: ChildStdin,
    output: BufReader<ChildStdout>,
    errors: Arc<Mutex<String>>,
}
impl Process {
    fn spawn(child: &Arc<Mutex<Option<Child>>>, online: bool) -> Result<Self> {
        if !installed() {
            bail!("Install the local runtime first using the Setup tab.");
        }
        let mut command = Command::new(python());
        command.arg("-u").arg(worker_dir().join("engine.py"));
        if online {
            command
                .args(["--online", "--once"])
                .env_remove("HF_HUB_OFFLINE")
                .env_remove("TRANSFORMERS_OFFLINE");
        }
        command
            .env("PYTHONUNBUFFERED", "1")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut process = command
            .spawn()
            .context("Could not start the MLX worker. Reinstall the runtime in Setup.")?;
        let input = process.stdin.take().unwrap();
        let output = BufReader::new(process.stdout.take().unwrap());
        let stderr = process.stderr.take().unwrap();
        let errors = Arc::new(Mutex::new(String::new()));
        let tail = errors.clone();
        thread::spawn(move || {
            for line in BufReader::new(stderr).lines().map_while(Result::ok) {
                // Keep a bounded diagnostic tail in RAM, never write transcripts/logs to disk.
                if let Ok(mut text) = tail.lock() {
                    *text = line.chars().take(1000).collect();
                }
            }
        });
        *child.lock().unwrap() = Some(process);
        Ok(Self {
            input,
            output,
            errors,
        })
    }
    fn request(&mut self, id: u64, mut value: Value, report: &impl Fn(String)) -> Result<Value> {
        value["id"] = json!(id);
        serde_json::to_writer(&mut self.input, &value)?;
        self.input.write_all(b"\n")?;
        self.input.flush()?;
        let mut line = String::new();
        loop {
            line.clear();
            if self.output.read_line(&mut line)? == 0 {
                let tail = self.errors.lock().unwrap().clone();
                bail!("The MLX worker stopped. {tail}");
            }
            let response: Value =
                serde_json::from_str(&line).context("Invalid response from MLX worker")?;
            if response.get("event").and_then(Value::as_str) == Some("progress") {
                report(
                    response["message"]
                        .as_str()
                        .unwrap_or("Working…")
                        .to_string(),
                );
                continue;
            }
            if response["id"].as_u64() != Some(id) {
                bail!("MLX response ID mismatch");
            }
            if response["ok"].as_bool() != Some(true) {
                bail!("{}", response["error"].as_str().unwrap_or("MLX failed"));
            }
            return Ok(response["result"].clone());
        }
    }
}
fn stop(child: &Arc<Mutex<Option<Child>>>) {
    if let Some(mut p) = child.lock().unwrap().take() {
        let _ = p.kill();
        let _ = p.wait();
    }
}
impl Service {
    pub fn new(repaint: impl Fn() + Send + Sync + 'static) -> Self {
        let (tx, requests) = mpsc::channel::<Request>();
        let (events, rx) = mpsc::channel();
        let child = Arc::new(Mutex::new(None));
        let shared = child.clone();
        let active = Arc::new(AtomicU64::new(0));
        let current = active.clone();
        thread::spawn(move || {
            let mut process: Option<Process> = None;
            while let Ok(request) = requests.recv() {
                let id = request.id;
                if current.load(Ordering::SeqCst) != id {
                    continue;
                }
                let report = |message: String| {
                    let _ = events.send(Event::Progress(id, message));
                    repaint();
                };
                let op = request.body["op"].as_str().unwrap_or("").to_string();
                let online = op == "download" || op == "catalog";
                let result = (|| -> Result<Value> {
                    if op == "setup" {
                        process = None;
                        stop(&shared);
                        return setup(&shared, &current, id, &report);
                    }
                    if online {
                        // No audio or transcript is sent to an online worker.
                        let clean = if op == "download" {
                            json!({"op": op, "repo": request.body["repo"]})
                        } else {
                            json!({"op": op})
                        };
                        process = None;
                        stop(&shared);
                        let mut online_process = Process::spawn(&shared, true)?;
                        if current.load(Ordering::SeqCst) != id {
                            bail!("Cancelled");
                        }
                        let result = online_process.request(id, clean, &report);
                        stop(&shared);
                        return result;
                    }
                    if shared.lock().unwrap().is_none() {
                        process = None;
                    }
                    if process.is_none() {
                        process = Some(Process::spawn(&shared, false)?);
                    }
                    if current.load(Ordering::SeqCst) != id {
                        bail!("Cancelled");
                    }
                    process.as_mut().unwrap().request(id, request.body, &report)
                })();
                match result {
                    Ok(value) => {
                        let _ = events.send(Event::Done(id, value));
                    }
                    Err(error) => {
                        process = None;
                        stop(&shared);
                        let _ = events.send(Event::Error(id, format!("{error:#}")));
                    }
                }
                repaint();
            }
            stop(&shared);
        });
        Self {
            tx,
            rx,
            child,
            active,
        }
    }
    pub fn submit(&self, id: u64, body: Value) {
        self.active.store(id, Ordering::SeqCst);
        let _ = self.tx.send(Request { id, body });
    }
    pub fn cancel(&self) {
        self.active.store(0, Ordering::SeqCst);
        stop(&self.child);
    }
}
impl Drop for Service {
    fn drop(&mut self) {
        self.cancel();
    }
}
