use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
};
use walkdir::WalkDir;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Speech,
    Rewrite,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Model {
    pub id: String,
    pub kind: Kind,
    #[serde(default)]
    pub path: Option<PathBuf>,
    #[serde(default)]
    pub bytes: u64,
    #[serde(default)]
    pub quant: String,
}
impl Model {
    pub fn installed(&self) -> bool {
        self.path.is_some()
    }
    pub fn title(&self) -> &str {
        self.id.rsplit('/').next().unwrap_or(&self.id)
    }
    pub fn estimated_gb(&self) -> f64 {
        let weights = if self.bytes > 0 {
            self.bytes as f64 / 1_073_741_824.0
        } else {
            estimate_weights(&self.id)
        };
        // Weight size alone is not an inference budget. Leave room for activations/KV.
        weights * 1.35 + if self.kind == Kind::Speech { 1.0 } else { 0.45 }
    }
    pub fn detail(&self) -> String {
        format!(
            "{} · {} · ~{:.1} GB memory",
            if self.installed() {
                "Downloaded"
            } else {
                "Available"
            },
            self.quant,
            self.estimated_gb()
        )
    }
}
pub fn quantization(id: &str, config: &Value) -> String {
    if let Some(n) = config
        .pointer("/quantization/bits")
        .or_else(|| config.pointer("/quantization_config/bits"))
        .and_then(Value::as_u64)
    {
        return format!("{n}-bit");
    }
    let name = id.to_lowercase();
    for (suffix, label) in [
        ("mxfp4", "MXFP4"),
        ("mxfp8", "MXFP8"),
        ("nvfp4", "NVFP4"),
        ("2bit", "2-bit"),
        ("3bit", "3-bit"),
        ("4bit", "4-bit"),
        ("6bit", "6-bit"),
        ("8bit", "8-bit"),
        ("-q4", "4-bit"),
        ("-q8", "8-bit"),
        ("bf16", "BF16"),
        ("fp16", "FP16"),
        ("fp32", "FP32"),
    ] {
        if name.contains(suffix) {
            return label.into();
        }
    }
    config
        .get("torch_dtype")
        .or_else(|| config.get("dtype"))
        .and_then(Value::as_str)
        .unwrap_or("Original")
        .to_string()
}
fn estimate_weights(id: &str) -> f64 {
    let s = id.to_lowercase();
    let params: f64 = if s.contains("s1-mini") {
        0.60
    } else if s.contains("whisper") {
        if s.contains("tiny") {
            0.04
        } else if s.contains("base") {
            0.08
        } else if s.contains("small") {
            0.25
        } else if s.contains("medium") {
            0.77
        } else if s.contains("turbo") {
            0.81
        } else {
            1.55
        }
    } else if s.contains("110m") {
        0.11
    } else if s.contains("0.6b") {
        0.6
    } else if s.contains("1.1b") {
        1.1
    } else if s.contains("1.5b") {
        1.5
    } else if s.contains("2.5b") {
        2.5
    } else if s.contains("3b") {
        3.0
    } else if s.contains("4b") {
        4.0
    } else if s.contains("7b") {
        7.0
    } else if s.contains("9b") || s.contains("vibevoice") {
        9.0
    } else {
        4.0
    };
    let q = quantization(id, &Value::Null);
    let bytes = match q.as_str() {
        "2-bit" => 0.35,
        "3-bit" => 0.45,
        "4-bit" | "MXFP4" | "NVFP4" => 0.6,
        "6-bit" => 0.85,
        "8-bit" | "MXFP8" => 1.1,
        "FP32" => 4.0,
        _ => 2.0,
    };
    params * bytes
}
fn kind_for(id: &str, config: &Value) -> Option<Kind> {
    let s = id.to_lowercase();
    if s.contains("s1-mini") && !s.contains("openaudio") && !s.contains("fish") {
        return Some(Kind::Rewrite);
    }
    let t = config
        .get("model_type")
        .and_then(Value::as_str)
        .unwrap_or("");
    if t == "whisper" && config.get("n_mels").is_some() {
        return Some(Kind::Speech);
    }
    let speech_types = [
        "parakeet",
        "qwen3_asr",
        "qwen2_audio",
        "voxtral",
        "voxtral_realtime",
        "cohere_asr",
        "fireredasr2",
        "sensevoice",
        "vibevoice_asr",
        "vibevoice",
        "glmasr",
        "glm_asr",
        "fun_asr_nano",
        "moonshine",
        "canary",
        "mms",
        "granite_speech",
        "granite_speech5_ctc",
        "granite_speech_nar",
        "moss_transcribe_diarize",
        "mega_asr",
    ];
    if speech_types.contains(&t) {
        return Some(Kind::Speech);
    }
    // Parakeet conversions also identify the architecture in model name/config.
    if s.contains("mlx")
        && [
            "parakeet",
            "qwen3-asr",
            "sensevoice",
            "voxtral-mini",
            "canary",
            "moonshine",
        ]
        .iter()
        .any(|key| s.contains(key))
        && !s.contains("tts")
        && !s.contains("forcedaligner")
    {
        return Some(Kind::Speech);
    }
    None
}
fn model_id(path: &Path) -> String {
    for component in path.components() {
        if let Some(name) = component
            .as_os_str()
            .to_str()
            .and_then(|s| s.strip_prefix("models--"))
        {
            return name.replacen("--", "/", 1);
        }
    }
    // Preserve a unique path for local conversions, including arbitrary quantizations.
    path.to_string_lossy().into_owned()
}
fn valid_weight(path: &Path) -> bool {
    fs::metadata(path).is_ok_and(|m| m.is_file() && m.len() > 0)
}
pub fn inspect_model(path: &Path) -> Option<Model> {
    let config: Value = serde_json::from_slice(&fs::read(path.join("config.json")).ok()?).ok()?;
    let id = model_id(path);
    let kind = kind_for(&id, &config)?;
    let weights: Vec<PathBuf> = fs::read_dir(path)
        .ok()?
        .filter_map(Result::ok)
        .map(|x| x.path())
        .filter(|p| {
            matches!(
                p.extension().and_then(|s| s.to_str()),
                Some("safetensors" | "npz")
            )
        })
        .collect();
    if weights.is_empty() || weights.iter().any(|p| !valid_weight(p)) {
        return None;
    }
    // A partial sharded download must never appear as a runnable model.
    for entry in fs::read_dir(path).ok()?.filter_map(Result::ok) {
        if entry
            .file_name()
            .to_string_lossy()
            .ends_with(".safetensors.index.json")
        {
            let index: Value = serde_json::from_slice(&fs::read(entry.path()).ok()?).ok()?;
            let names: BTreeSet<&str> = index
                .get("weight_map")?
                .as_object()?
                .values()
                .filter_map(Value::as_str)
                .collect();
            if names.is_empty()
                || names.iter().any(|name| {
                    let p = Path::new(name);
                    p.components().count() != 1 || !valid_weight(&path.join(p))
                })
            {
                return None;
            }
        }
    }
    if kind == Kind::Rewrite && !path.join("tokenizer.json").is_file() {
        return None;
    }
    let bytes = weights
        .iter()
        .filter_map(|p| fs::metadata(p).ok())
        .map(|m| m.len())
        .sum();
    Some(Model {
        quant: quantization(&id, &config),
        id,
        kind,
        path: Some(path.to_path_buf()),
        bytes,
    })
}
pub fn cache_roots(extra: &[PathBuf]) -> Vec<PathBuf> {
    let home = directories::BaseDirs::new()
        .expect("home directory")
        .home_dir()
        .to_path_buf();
    let hf_home = std::env::var_os("HF_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            std::env::var_os("XDG_CACHE_HOME")
                .map(PathBuf::from)
                .unwrap_or_else(|| home.join(".cache"))
                .join("huggingface")
        });
    let hub = std::env::var_os("HF_HUB_CACHE")
        .or_else(|| std::env::var_os("HUGGINGFACE_HUB_CACHE"))
        .map(PathBuf::from)
        .unwrap_or_else(|| hf_home.join("hub"));
    let mut roots = vec![
        hub,
        home.join(".cache/huggingface/hub"),
        home.join(".lmstudio/models"),
        home.join("Models"),
    ];
    roots.extend_from_slice(extra);
    roots.sort();
    roots.dedup();
    roots
}
pub fn scan(roots: &[PathBuf]) -> Vec<Model> {
    let mut found = BTreeMap::new();
    for root in roots {
        for entry in WalkDir::new(root)
            .max_depth(7)
            .into_iter()
            .filter_entry(|e| {
                !e.file_type().is_dir()
                    || !["blobs", ".git", ".venv", "target", "node_modules", ".locks"]
                        .contains(&e.file_name().to_string_lossy().as_ref())
            })
            .filter_map(Result::ok)
        {
            if entry.file_name() == "config.json"
                && let Some(model) = entry.path().parent().and_then(inspect_model)
            {
                // Prefer the snapshot refs/main points to, when multiple revisions exist.
                let preferred = entry.path().parent().is_some_and(|p| {
                    p.parent()
                        .and_then(Path::parent)
                        .and_then(|repo| fs::read_to_string(repo.join("refs/main")).ok())
                        .is_some_and(|r| p.file_name().is_some_and(|n| n == r.trim()))
                });
                if preferred || !found.contains_key(&model.id) {
                    found.insert(model.id.clone(), model);
                }
            }
        }
    }
    found.into_values().collect()
}
pub fn all_models(extra: &[PathBuf]) -> Vec<Model> {
    let mut available: Vec<Model> =
        serde_json::from_str(include_str!("../assets/catalog.json")).unwrap_or_default();
    if let Ok(data) = fs::read(crate::settings::data_dir().join("catalog.json"))
        && let Ok(remote) = serde_json::from_slice::<Vec<Model>>(&data)
    {
        available.extend(remote);
    }
    merge(available, scan(&cache_roots(extra)))
}
pub fn merge(available: Vec<Model>, installed: Vec<Model>) -> Vec<Model> {
    let mut map: BTreeMap<String, Model> = available
        .into_iter()
        .map(|mut m| {
            m.path = None;
            m.quant = quantization(&m.id, &Value::Null);
            (m.id.clone(), m)
        })
        .collect();
    for m in installed {
        map.insert(m.id.clone(), m);
    }
    let mut result: Vec<Model> = map.into_values().collect();
    result.sort_by(|a, b| b.installed().cmp(&a.installed()).then(a.id.cmp(&b.id)));
    result
}
#[derive(Clone, Copy, Debug)]
pub struct Memory {
    pub total_gb: f64,
    pub available_gb: f64,
}
impl Memory {
    pub fn read() -> Self {
        let mut system = sysinfo::System::new();
        system.refresh_memory();
        Self {
            total_gb: system.total_memory() as f64 / 1_073_741_824.0,
            available_gb: system.available_memory() as f64 / 1_073_741_824.0,
        }
    }
    pub fn budget(&self) -> f64 {
        (self.total_gb * 0.6).min((self.available_gb - 1.0).max(0.0))
    }
}
pub fn recommend(models: &[Model], kind: Kind, memory: Memory) -> Option<&Model> {
    models
        .iter()
        .filter(|m| m.kind == kind && m.estimated_gb() <= memory.budget())
        .max_by_key(|m| {
            let name = m.id.to_lowercase();
            let base = if kind == Kind::Rewrite {
                if name.ends_with("8bit") && memory.total_gb >= 16.0 {
                    90
                } else if name.ends_with("4bit") {
                    80
                } else {
                    30
                }
            } else if name.contains("parakeet-tdt-0.6b-v3") {
                100
            } else if name.contains("qwen3-asr-0.6b") {
                95
            } else if name.contains("whisper-large-v3-turbo") {
                90
            } else if name.contains("whisper-small") {
                70
            } else if name.contains("whisper-tiny") {
                50
            } else {
                20
            };
            base + if m.installed() { 1000 } else { 0 }
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture(config: &str) -> tempfile::TempDir {
        let d = tempfile::tempdir().unwrap();
        fs::write(d.path().join("config.json"), config).unwrap();
        d
    }
    #[test]
    fn ignores_metadata_only_and_pytorch_whisper() {
        let d = fixture(r#"{"model_type":"whisper","n_mels":80}"#);
        assert!(inspect_model(d.path()).is_none());
        fs::write(d.path().join("weights.npz"), b"weights").unwrap();
        assert!(inspect_model(d.path()).is_some());
        fs::write(
            d.path().join("config.json"),
            r#"{"model_type":"whisper","d_model":384}"#,
        )
        .unwrap();
        assert!(inspect_model(d.path()).is_none());
    }
    #[test]
    fn rejects_missing_shards_and_path_traversal() {
        let d = fixture(r#"{"model_type":"qwen3_asr"}"#);
        fs::write(d.path().join("one.safetensors"), b"weights").unwrap();
        fs::write(
            d.path().join("model.safetensors.index.json"),
            r#"{"weight_map":{"a":"one.safetensors","b":"two.safetensors"}}"#,
        )
        .unwrap();
        assert!(inspect_model(d.path()).is_none());
        fs::write(d.path().join("two.safetensors"), b"weights").unwrap();
        assert!(inspect_model(d.path()).is_some());
        fs::write(
            d.path().join("model.safetensors.index.json"),
            r#"{"weight_map":{"a":"../one.safetensors"}}"#,
        )
        .unwrap();
        assert!(inspect_model(d.path()).is_none());
    }
    #[test]
    fn no_recommendation_under_pressure() {
        let models = vec![Model {
            id: "mlx-community/S1-mini-MLX-4bit".into(),
            kind: Kind::Rewrite,
            path: None,
            bytes: 0,
            quant: "4-bit".into(),
        }];
        assert!(
            recommend(
                &models,
                Kind::Rewrite,
                Memory {
                    total_gb: 8.0,
                    available_gb: 0.5
                }
            )
            .is_none()
        );
    }
    #[test]
    fn recommends_downloaded_models_and_quant_from_config() {
        let mut models = vec![Model {
            id: "mlx-community/S1-mini-MLX-4bit".into(),
            kind: Kind::Rewrite,
            path: Some("/test".into()),
            bytes: 0,
            quant: "4-bit".into(),
        }];
        models.push(Model {
            id: "mlx-community/S1-mini-MLX-8bit".into(),
            path: None,
            ..models[0].clone()
        });
        let selected = recommend(
            &models,
            Kind::Rewrite,
            Memory {
                total_gb: 64.0,
                available_gb: 40.0,
            },
        )
        .unwrap();
        assert!(selected.installed());
        assert_eq!(
            quantization("custom", &serde_json::json!({"quantization": {"bits": 6}})),
            "6-bit"
        );
    }
}
