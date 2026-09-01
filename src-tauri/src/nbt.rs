// 对应 Nbt.java：极简 NBT 解析器（读取 level.dat 元数据 + 结构校验）。

use crate::error::{conv, ConversionError, Result};
use flate2::read::GzDecoder;
use std::io::{BufReader, Read};

const MAX_DEPTH: usize = 512;
const MAX_COLLECTION_LENGTH: i64 = 128 * 1024 * 1024;

use crate::models::LevelMetadata;

#[allow(dead_code)] // Float/Bytes/List 仅在收集模式下存值，当前只用元数据访问器
enum Value {
    Number(i64),
    Float(f64),
    Bytes(Vec<u8>),
    Str(String),
    List(Vec<Value>),
    Compound(Vec<(String, Value)>),
    Skipped,
}

impl Value {
    fn as_i64(&self) -> Option<i64> {
        match self {
            Value::Number(n) => Some(*n),
            _ => None,
        }
    }

    fn as_str(&self) -> Option<&str> {
        match self {
            Value::Str(s) => Some(s),
            _ => None,
        }
    }

    fn get(&self, name: &str) -> Option<&Value> {
        match self {
            Value::Compound(entries) => entries.iter().find(|(k, _)| k == name).map(|(_, v)| v),
            _ => None,
        }
    }
}

struct Reader<R: Read> {
    inner: R,
    collect: bool,
}

impl<R: Read> Reader<R> {
    fn u8(&mut self) -> Result<u8> {
        let mut buffer = [0u8; 1];
        self.inner
            .read_exact(&mut buffer)
            .map_err(|_| ConversionError::from(self.eof_message()))?;
        Ok(buffer[0])
    }

    fn eof_message(&self) -> &'static str {
        if self.collect {
            "level.dat NBT 被截断"
        } else {
            "NBT 数据被截断"
        }
    }

    fn bytes(&mut self, count: usize) -> Result<Vec<u8>> {
        let mut buffer = vec![0u8; count];
        self.inner
            .read_exact(&mut buffer)
            .map_err(|_| ConversionError::from(self.eof_message()))?;
        Ok(buffer)
    }

    fn i32_be(&mut self) -> Result<i32> {
        let b = self.bytes(4)?;
        Ok(i32::from_be_bytes([b[0], b[1], b[2], b[3]]))
    }

    fn i16_be(&mut self) -> Result<i16> {
        let b = self.bytes(2)?;
        Ok(i16::from_be_bytes([b[0], b[1]]))
    }

    fn i64_be(&mut self) -> Result<i64> {
        let b = self.bytes(8)?;
        Ok(i64::from_be_bytes([
            b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7],
        ]))
    }

    fn string(&mut self) -> Result<String> {
        let length = self.i16_be()? as u16 as usize;
        let bytes = self.bytes(length)?;
        Ok(String::from_utf8_lossy(&bytes).to_string())
    }

    fn checked_length(&self, length: i32, element_bytes: i64) -> Result<i64> {
        if length < 0 {
            return conv(format!("NBT 集合长度为负数：{length}"));
        }
        if length as i64 * element_bytes > MAX_COLLECTION_LENGTH {
            return conv(format!("NBT 集合异常大：{length} 项"));
        }
        Ok(length as i64)
    }

    fn skip(&mut self, mut length: u64) -> Result<()> {
        let mut buffer = [0u8; 8192];
        while length > 0 {
            let chunk = length.min(buffer.len() as u64) as usize;
            self.inner
                .read_exact(&mut buffer[..chunk])
                .map_err(|_| ConversionError::from(self.eof_message()))?;
            length -= chunk as u64;
        }
        Ok(())
    }

    fn payload(&mut self, tag_type: u8, depth: usize) -> Result<Value> {
        if depth > MAX_DEPTH {
            return conv(format!("NBT 嵌套超过 {MAX_DEPTH} 层"));
        }
        Ok(match tag_type {
            1 => Value::Number(self.u8()? as i8 as i64),
            2 => Value::Number(self.i16_be()? as i64),
            3 => Value::Number(self.i32_be()? as i64),
            4 => Value::Number(self.i64_be()?),
            5 => {
                let b = self.bytes(4)?;
                Value::Float(f32::from_be_bytes([b[0], b[1], b[2], b[3]]) as f64)
            }
            6 => {
                let b = self.bytes(8)?;
                Value::Float(f64::from_be_bytes([
                    b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7],
                ]))
            }
            7 => {
                let raw_length = self.i32_be()?;
                let length = self.checked_length(raw_length, 1)?;
                if self.collect && length <= 1024 * 1024 {
                    Value::Bytes(self.bytes(length as usize)?)
                } else {
                    self.skip(length as u64)?;
                    Value::Skipped
                }
            }
            8 => Value::Str(self.string()?),
            9 => {
                let child_type = self.u8()?;
                let raw_length = self.i32_be()?;
                let length = self.checked_length(raw_length, 1)?;
                if child_type == 0 && length != 0 {
                    return conv("非空 NBT List 使用 TAG_End 类型");
                }
                let mut list = Vec::new();
                for _ in 0..length {
                    let value = self.payload(child_type, depth + 1)?;
                    if self.collect {
                        list.push(value);
                    }
                }
                if self.collect {
                    Value::List(list)
                } else {
                    Value::Skipped
                }
            }
            10 => {
                let mut compound = Vec::new();
                loop {
                    let child_type = self.u8()?;
                    if child_type == 0 {
                        break;
                    }
                    if !(1..=12).contains(&child_type) {
                        return conv(format!("非法 NBT 标签类型：{child_type}"));
                    }
                    let name = self.string()?;
                    let value = self.payload(child_type, depth + 1)?;
                    if self.collect {
                        compound.push((name, value));
                    }
                }
                if self.collect {
                    Value::Compound(compound)
                } else {
                    Value::Skipped
                }
            }
            11 => {
                let raw_length = self.i32_be()?;
                let length = self.checked_length(raw_length, 4)?;
                self.skip((length * 4) as u64)?;
                Value::Skipped
            }
            12 => {
                let raw_length = self.i32_be()?;
                let length = self.checked_length(raw_length, 8)?;
                self.skip((length * 8) as u64)?;
                Value::Skipped
            }
            other => return conv(format!("非法 NBT 标签类型：{other}")),
        })
    }
}

pub fn read_java_level(level_dat: &std::path::Path) -> Result<LevelMetadata> {
    let file = std::fs::File::open(level_dat)?;
    let mut reader = Reader {
        inner: GzDecoder::new(BufReader::new(file)),
        collect: true,
    };
    let root_type = reader.u8()?;
    if root_type != 10 {
        return conv(format!(
            "level.dat 根标签类型为 {root_type}，预期 Compound(10)"
        ));
    }
    let _root_name = reader.string()?;
    let root = reader.payload(root_type, 0)?;
    let Value::Compound(_) = &root else {
        return conv("level.dat 根标签不是 Compound");
    };
    let data = root.get("Data").unwrap_or(&root);
    let data_version = data
        .get("DataVersion")
        .and_then(|v| v.as_i64())
        .unwrap_or(-1) as i32;
    let world_name = data
        .get("LevelName")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let version_name = data
        .get("Version")
        .and_then(|v| v.get("Name"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    Ok(LevelMetadata {
        data_version,
        version_name,
        world_name,
    })
}

/// 校验一棵完整的 NBT Compound 树（输入应为已解压的原始 NBT 字节流）。
pub fn validate_root(source: impl Read) -> Result<()> {
    let mut reader = Reader {
        inner: source,
        collect: false,
    };
    let root_type = reader.u8()?;
    if root_type != 10 {
        return conv(format!("NBT 根标签类型为 {root_type}，预期 Compound(10)"));
    }
    let _name = reader.string()?;
    reader.payload(root_type, 0)?;
    Ok(())
}
