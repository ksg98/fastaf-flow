//! Run explicitly on an Apple Silicon Mac after `uv sync --project worker`.
use fastaf_flow::{
    catalog::{self, Kind},
    runtime::{Event, Service},
};
use serde_json::{Value, json};
use std::time::{Duration, Instant};

fn response(service: &Service, id: u64) -> Result<Value, String> {
    let deadline = Instant::now() + Duration::from_secs(180);
    while Instant::now() < deadline {
        match service.rx.recv_timeout(Duration::from_secs(5)) {
            Ok(Event::Done(i, value)) if i == id => return Ok(value),
            Ok(Event::Error(i, error)) if i == id => return Err(error),
            Ok(_) | Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(error) => return Err(error.to_string()),
        }
    }
    panic!("Worker request {id} timed out");
}

#[test]
#[ignore = "requires Apple Silicon, local MLX runtime, model weights, and the synthetic WAV fixture"]
fn local_pipeline_recovers_after_errors_and_cancellation() {
    let models = catalog::all_models(&[]);
    let speech = models
        .iter()
        .find(|m| m.installed() && m.id == "mlx-community/Qwen3-ASR-0.6B-4bit")
        .expect("download Qwen3-ASR 0.6B 4-bit");
    let rewrite = models
        .iter()
        .find(|m| m.installed() && m.kind == Kind::Rewrite)
        .expect("download S1-mini");
    let audio =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(".test-artifacts/dictation.wav");
    assert!(
        audio.is_file(),
        "generate the synthetic WAV with scripts/smoke-test.sh"
    );
    let service = Service::new(|| {});
    service.submit(1, json!({"op": "ping"}));
    assert_eq!(response(&service, 1).unwrap()["ready"], true);
    service.submit(
        2,
        json!({"op":"rewrite", "model":"/nonexistent-fastaf-model", "text":"test"}),
    );
    assert!(response(&service, 2).is_err());
    service.submit(
        3,
        json!({"op":"transcribe", "model": speech.path, "audio":audio}),
    );
    std::thread::sleep(Duration::from_millis(100));
    service.cancel();
    service.submit(4, json!({"op": "ping"}));
    assert_eq!(response(&service, 4).unwrap()["ready"], true);
    service.submit(
        5,
        json!({"op":"transcribe", "model":speech.path, "audio":audio, "language":"en"}),
    );
    let transcript = response(&service, 5).unwrap();
    assert!(
        transcript["text"]
            .as_str()
            .unwrap()
            .to_lowercase()
            .contains("thursday")
    );
    service.submit(
        6,
        json!({"op":"rewrite", "model":rewrite.path, "text":transcript["text"]}),
    );
    let cleaned = response(&service, 6).unwrap();
    let text = cleaned["text"].as_str().unwrap().to_lowercase();
    assert!(text.contains("thursday"), "{text}");
    assert!(!text.contains("friday"), "{text}");
    assert!(text.contains("budget"), "{text}");
    service.submit(
        7,
        json!({"op":"rewrite", "model":rewrite.path, "text":"um"}),
    );
    assert_eq!(response(&service, 7).unwrap()["text"], "");
    service.submit(8, json!({"op":"unload"}));
    assert_eq!(response(&service, 8).unwrap()["unloaded"], true);
}
