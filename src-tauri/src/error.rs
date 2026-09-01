// 统一错误类型：对应 Java 版 ConversionException。
// 结构化 code 供前端做精确分类（如取消判定），message 供展示与日志。

/// 默认错误码：一般失败。
pub const CODE_ERROR: &str = "error";
/// 用户主动取消。
pub const CODE_CANCELLED: &str = "cancelled";
/// 后端无输出心跳超时。
pub const CODE_TIMEOUT: &str = "timeout";

#[derive(Debug, Clone)]
pub struct ConversionError {
    pub code: &'static str,
    pub message: String,
}

impl ConversionError {
    pub fn new(code: &'static str, message: impl Into<String>) -> Self {
        ConversionError {
            code,
            message: message.into(),
        }
    }

    /// 序列化为 IPC 错误负载（前端 parseError 解析）。
    pub fn to_ipc(&self) -> String {
        serde_json::json!({ "code": self.code, "message": self.message }).to_string()
    }
}

impl std::fmt::Display for ConversionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for ConversionError {}

impl From<std::io::Error> for ConversionError {
    fn from(value: std::io::Error) -> Self {
        ConversionError::new(CODE_ERROR, value.to_string())
    }
}

impl From<zip::result::ZipError> for ConversionError {
    fn from(value: zip::result::ZipError) -> Self {
        ConversionError::new(CODE_ERROR, value.to_string())
    }
}

impl From<String> for ConversionError {
    fn from(message: String) -> Self {
        ConversionError::new(CODE_ERROR, message)
    }
}

impl From<&str> for ConversionError {
    fn from(message: &str) -> Self {
        ConversionError::new(CODE_ERROR, message)
    }
}

pub type Result<T> = std::result::Result<T, ConversionError>;

pub fn conv<T>(message: impl Into<String>) -> Result<T> {
    Err(ConversionError::new(CODE_ERROR, message))
}

pub fn conv_code<T>(code: &'static str, message: impl Into<String>) -> Result<T> {
    Err(ConversionError::new(code, message))
}
