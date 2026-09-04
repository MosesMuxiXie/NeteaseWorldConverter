// 数据模型：内部结构（对应 Models.java）与前端 DTO。

use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum WorldType {
    NeteaseBedrock,
    NeteaseBedrockLegacyAes,
    Bedrock,
    Java,
}

impl WorldType {
    pub fn display_name(&self) -> &'static str {
        match self {
            WorldType::NeteaseBedrock => "网易基岩版（可解密）",
            WorldType::NeteaseBedrockLegacyAes => "网易基岩版（旧 AES 加密）",
            WorldType::Bedrock => "标准基岩版",
            WorldType::Java => "Java 版（含网易 Java）",
        }
    }

    pub fn is_bedrock(&self) -> bool {
        *self != WorldType::Java
    }
}

#[derive(Debug, Clone)]
pub struct WorldInfo {
    pub root: PathBuf,
    pub database_directory: Option<PathBuf>,
    pub world_type: WorldType,
    pub detected_version: String,
    pub world_name: String,
    pub file_count: u64,
    pub byte_count: u64,
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct TargetVersion {
    #[serde(rename = "displayName")]
    pub display_name: String,
    pub chunker_format: String,
}

#[derive(Debug, Clone)]
pub struct LevelMetadata {
    pub data_version: i32,
    pub version_name: String,
    pub world_name: String,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct ValidationResult {
    pub region_files: i64,
    pub region_chunks: i64,
    pub files: i64,
    pub bytes: i64,
    pub level_version: String,
    pub data_version: i32,
}

// ---------- 前端 DTO ----------

#[derive(Debug, Clone, serde::Serialize)]
pub struct AnalysisDto {
    #[serde(rename = "sessionId")]
    pub session_id: String,
    #[serde(rename = "typeName")]
    pub type_name: String,
    #[serde(rename = "detectedVersion")]
    pub detected_version: String,
    #[serde(rename = "worldName")]
    pub world_name: String,
    #[serde(rename = "fileCount")]
    pub file_count: u64,
    #[serde(rename = "byteCount")]
    pub byte_count: u64,
    pub notes: Vec<String>,
    pub supported: bool,
    pub targets: Vec<TargetVersion>,
    #[serde(rename = "sourceVersion")]
    pub source_version: String,
    #[serde(rename = "errorReport")]
    pub error_report: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct ConversionResultDto {
    #[serde(rename = "resultZip")]
    pub result_zip: String,
    #[serde(rename = "fileName")]
    pub file_name: String,
    #[serde(rename = "targetVersion")]
    pub target_version: String,
    #[serde(rename = "regionFiles")]
    pub region_files: i64,
    #[serde(rename = "regionChunks")]
    pub region_chunks: i64,
    #[serde(rename = "regionNote")]
    pub region_note: String,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct BackendStatusDto {
    pub ok: bool,
    pub java: String,
    pub chunker: String,
    pub b2j: String,
    pub message: String,
}

#[derive(Clone, serde::Serialize)]
pub struct ResourceUsageDto {
    #[serde(rename = "cpuPercent")]
    pub cpu_percent: f32,
    #[serde(rename = "memoryBytes")]
    pub memory_bytes: u64,
}

#[derive(Clone, serde::Serialize)]
pub struct ProgressPayload {
    #[serde(rename = "sessionId")]
    pub session_id: String,
    pub percent: u32,
    pub stage: String,
    pub detail: String,
}

#[derive(Clone, serde::Serialize)]
pub struct LogPayload {
    #[serde(rename = "sessionId")]
    pub session_id: String,
    pub line: String,
}
