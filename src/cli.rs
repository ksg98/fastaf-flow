use anyhow::{Result, bail};
use fastaf_flow::{
    catalog::{self, Kind, Memory},
    runtime::{Event, Service},
    settings::Settings,
};
use serde_json::{Value, json};
use std::time::{Duration, Instant};

fn request(service: &Service, id: u64, body: Value) -> Result<Value> {
    let timeout = if body["op"] == "setup" { 3600 } else { 600 };
    service.submit(id, body);
    let deadline = Instant::now() + Duration::from_secs(timeout);
    while Instant::now() < deadline {
        match service.rx.recv_timeout(Duration::from_secs(1)) {
            Ok(Event::Done(i, value)) if i == id => return Ok(value),
            Ok(Event::Error(i, error)) if i == id => bail!("{error}"),
            Ok(Event::Progress(_, message)) => eprintln!("{message}"),
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                bail!("MLX worker disconnected")
            }
            _ => {}
        }
    }
    service.cancel();
    bail!("MLX operation timed out")
}
pub fn run(args: &[String]) -> Result<Value> {
    let settings = Settings::load()?;
    let service = Service::new(|| {});
    match args[1].as_str() {
        "--setup-runtime" => request(&service, 1, json!({"op":"setup"})),
        "--doctor" => {
            let engine = request(&service, 1, json!({"op":"ping"}))?;
            let memory = Memory::read();
            Ok(
                json!({"engine":engine, "data_dir":fastaf_flow::settings::data_dir(),
                "total_memory_gb":memory.total_gb, "available_memory_gb":memory.available_gb}),
            )
        }
        "--transcribe" => {
            if args.len() != 3 {
                bail!("Usage: fastaf-flow --transcribe /path/to/audio.wav");
            }
            let audio = std::fs::canonicalize(&args[2])?;
            let models = catalog::all_models(&settings.model_roots);
            let memory = Memory::read();
            let speech = models
                .iter()
                .find(|m| m.installed() && m.id == settings.speech_model)
                .or_else(|| {
                    catalog::recommend(&models, Kind::Speech, memory).filter(|m| m.installed())
                })
                .ok_or_else(|| {
                    anyhow::anyhow!("Download a compatible speech model in the app first.")
                })?;
            let result = request(
                &service,
                1,
                json!({"op":"transcribe", "model":speech.path,
                "audio":audio, "language":settings.language, "keep_models":false}),
            )?;
            let raw = result["text"].as_str().unwrap_or("");
            let mut text = raw.to_string();
            let detected = result["language"].as_str().unwrap_or("en").to_lowercase();
            if settings.auto_rewrite
                && !raw.is_empty()
                && ["auto", "en"].contains(&settings.language.as_str())
                && ["en", "english"].contains(&detected.as_str())
            {
                let rewrite = models
                    .iter()
                    .find(|m| m.installed() && m.id == settings.rewrite_model)
                    .or_else(|| {
                        catalog::recommend(&models, Kind::Rewrite, memory).filter(|m| m.installed())
                    })
                    .ok_or_else(|| {
                        anyhow::anyhow!("Download S1-mini by Superwhisper in the app first.")
                    })?;
                let rewritten = request(
                    &service,
                    2,
                    json!({"op":"rewrite", "model":rewrite.path, "text":raw,
                    "styling":settings.styling, "structure":settings.structure, "context":settings.context, "keep_models":false}),
                )?;
                text = rewritten["text"].as_str().unwrap_or("").to_string();
            }
            Ok(json!({"raw":raw, "text":text, "speech_model":speech.id}))
        }
        _ => bail!(
            "Options: --version, --list-models, --doctor, --setup-runtime, --transcribe audio.wav"
        ),
    }
}
