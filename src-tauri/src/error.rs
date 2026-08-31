// 统一错误类型：对应 Java 版 ConversionException。

#[derive(Debug, Clone)]
pub struct ConversionError(pub String);

impl std::fmt::Display for ConversionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for ConversionError {}

impl From<std::io::Error> for ConversionError {
    fn from(value: std::io::Error) -> Self {
        ConversionError(value.to_string())
    }
}

impl From<zip::result::ZipError> for ConversionError {
    fn from(value: zip::result::ZipError) -> Self {
        ConversionError(value.to_string())
    }
}

pub type Result<T> = std::result::Result<T, ConversionError>;

pub fn conv<T>(message: impl Into<String>) -> Result<T> {
    Err(ConversionError(message.into()))
}
