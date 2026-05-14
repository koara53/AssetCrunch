use std::collections::HashMap;
use serde::{Serialize, Deserialize};

#[derive(Serialize, Deserialize, Clone)]
pub struct FileEntry {
    pub original: String,
    pub output: String,
    #[serde(rename = "type")]
    pub file_type: String,
    pub method: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    pub original_size: u64,
    pub compressed_size: u64,
    pub reduction: f64,
}

#[derive(Serialize, Deserialize)]
pub struct Manifest {
    pub version: String,
    pub input_dir: String,
    pub output_dir: String,
    pub files: Vec<FileEntry>,
    pub summary: Summary,
}

#[derive(Serialize, Deserialize)]
pub struct Summary {
    pub total_files: usize,
    pub compressed_files: usize,
    pub skipped_files: usize,
    pub copied_files: usize,
    pub original_size: u64,
    pub output_size: u64,
    pub saved_bytes: i64,
    pub reduction: f64,
}

impl Manifest {
    pub fn new(input_dir: &str, output_dir: &str) -> Self {
        Self {
            version: "0.2.0".into(),
            input_dir: input_dir.to_string(),
            output_dir: output_dir.to_string(),
            files: Vec::new(),
            summary: Summary {
                total_files: 0,
                compressed_files: 0,
                skipped_files: 0,
                copied_files: 0,
                original_size: 0,
                output_size: 0,
                saved_bytes: 0,
                reduction: 0.0,
            },
        }
    }

    pub fn add_compressed(&mut self, original: &str, output: &str,
        file_type: &str, method: &str,
        original_size: u64, compressed_size: u64)
    {
        let reduction = (1.0 - compressed_size as f64 / original_size as f64) * 100.0;
        self.files.push(FileEntry {
            original: original.to_string(),
            output: output.to_string(),
            file_type: file_type.to_string(),
            method: method.to_string(),
            reason: None,
            original_size,
            compressed_size,
            reduction,
        });
        self.summary.compressed_files += 1;
    }

    pub fn add_skipped(&mut self, original: &str, output: &str,
        file_type: &str, reason: &str, size: u64)
    {
        self.files.push(FileEntry {
            original: original.to_string(),
            output: output.to_string(),
            file_type: file_type.to_string(),
            method: "copy".to_string(),
            reason: Some(reason.to_string()),
            original_size: size,
            compressed_size: size,
            reduction: 0.0,
        });
        self.summary.copied_files += 1;
    }

    pub fn finalize(&mut self) {
        self.summary.total_files = self.files.len();
        self.summary.original_size = self.files.iter()
            .map(|f| f.original_size).sum();
        self.summary.output_size = self.files.iter()
            .map(|f| f.compressed_size).sum();
        self.summary.saved_bytes =
            self.summary.original_size as i64 - self.summary.output_size as i64;
        self.summary.reduction = if self.summary.original_size > 0 {
            (1.0 - self.summary.output_size as f64
                / self.summary.original_size as f64) * 100.0
        } else { 0.0 };
    }

pub fn save(&self, path: &str) {
        let json = serde_json::to_string_pretty(self).expect("JSON変換失敗");
        // UTF-8 BOM付きで書き込む（Windowsでの文字化け防止）
        let mut bytes = vec![0xEF, 0xBB, 0xBF]; // UTF-8 BOM
        bytes.extend_from_slice(json.as_bytes());
        std::fs::write(path, bytes).expect("マニフェスト書き込み失敗");
    }

pub fn load(path: &str) -> Result<Self, String> {
        let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
        // UTF-8 BOMをスキップ
        let data = if bytes.starts_with(&[0xEF, 0xBB, 0xBF]) {
            &bytes[3..]
        } else {
            &bytes[..]
        };
        let text = std::str::from_utf8(data).map_err(|e| e.to_string())?;
        serde_json::from_str(text).map_err(|e| e.to_string())
    }
}
    