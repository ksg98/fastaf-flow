use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::{fs, io::Write, path::PathBuf};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub speech_model: String,
    pub rewrite_model: String,
    pub auto_rewrite: bool,
    pub auto_paste: bool,
    pub hold_to_talk: bool,
    pub styling: String,
    pub structure: String,
    pub context: String,
    pub language: String,
    pub model_roots: Vec<PathBuf>,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            speech_model: String::new(),
            rewrite_model: String::new(),
            auto_rewrite: true,
            auto_paste: false,
            hold_to_talk: false,
            styling: "semi-formal".into(),
            structure: "prose".into(),
            context: "general".into(),
            language: "auto".into(),
            model_roots: vec![],
        }
    }
}
pub fn data_dir() -> PathBuf {
    if let Some(path) = std::env::var_os("FASTAF_DATA_DIR") {
        return path.into();
    }
    directories::ProjectDirs::from("dev", "fastaf", "FastAF Flow")
        .expect("a home directory is required")
        .data_local_dir()
        .to_path_buf()
}
impl Settings {
    pub fn load() -> Result<Self> {
        let path = data_dir().join("settings.json");
        if !path.exists() {
            return Ok(Self::default());
        }
        let mut settings: Self = serde_json::from_slice(&fs::read(&path)?)
            .context("Settings could not be read. The file has been preserved.")?;
        if !["casual", "semi-casual", "semi-formal", "formal"].contains(&settings.styling.as_str())
        {
            settings.styling = "semi-formal".into();
        }
        if !["prose", "lists"].contains(&settings.structure.as_str()) {
            settings.structure = "prose".into();
        }
        if !["general", "email"].contains(&settings.context.as_str()) {
            settings.context = "general".into();
        }
        Ok(settings)
    }
    pub fn save(&self) -> Result<()> {
        fs::create_dir_all(data_dir())?;
        let mut file = tempfile::NamedTempFile::new_in(data_dir())?;
        file.write_all(&serde_json::to_vec_pretty(self)?)?;
        file.as_file().sync_all()?;
        file.persist(data_dir().join("settings.json"))?;
        Ok(())
    }
}
