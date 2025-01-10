use crate::{
    util::{IOError, IOResult},
    version,
};
use byteorder::{ReadBytesExt, WriteBytesExt, BE};
use cesu8::to_java_cesu8;
use either::Either;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Number, Value};
use std::{collections::HashMap, fmt::Debug, io::ErrorKind};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum NbtType {
    ByteTag(i8),
    ShortTag(i16),
    IntTag(i32),
    LongTag(i64),
    FloatTag(f32),
    DoubleTag(f64),
    ByteArrayTag(Vec<i8>),
    StringTag(String),
    ListTag(i8, Vec<NbtType>),
    CompoundTag(HashMap<String, NbtType>),
    IntArrayTag(Vec<i32>),
    LongArrayTag(Vec<i64>),
}

impl NbtType {
    pub fn from_json(json: &Value) -> IOResult<Self> {
        Ok(match json {
            Value::Number(value) => {
                if let Some(value) = value.as_f64() {
                    if value >= f32::MIN as f64 && value <= f32::MAX as f64 {
                        Self::FloatTag(value as f32)
                    } else {
                        Self::DoubleTag(value)
                    }
                } else {
                    if let Some(value) = value.as_i64() {
                        if value >= i8::MIN as i64 && value <= i8::MAX as i64 {
                            Self::ByteTag(value as i8)
                        } else if value >= i16::MIN as i64 && value <= i16::MAX as i64 {
                            Self::ShortTag(value as i16)
                        } else if value >= i32::MIN as i64 && value <= i32::MAX as i64 {
                            Self::IntTag(value as i32)
                        } else {
                            Self::LongTag(value)
                        }
                    } else {
                        return Err(IOError::new(
                            ErrorKind::InvalidData,
                            "json number is not i64 or f64",
                        ));
                    }
                }
            }
            Value::String(value) => Self::StringTag(value.to_owned()),
            Value::Bool(value) => {
                if *value {
                    Self::ByteTag(1)
                } else {
                    Self::ByteTag(0)
                }
            }
            Value::Array(values) => {
                if values.is_empty() {
                    Self::ListTag(0, Vec::new())
                } else {
                    let id = Self::from_json(values.first().unwrap())?.id();
                    match id {
                        1 => {
                            let mut bytes = Vec::new();
                            for byte in values {
                                if let Some(byte) = byte.as_i64() {
                                    bytes.push(byte as i8);
                                } else {
                                    return Err(IOError::new(
                                        ErrorKind::InvalidData,
                                        "not a i8 in i8 array",
                                    ));
                                }
                            }
                            Self::ByteArrayTag(bytes)
                        }
                        3 => {
                            let mut ints = Vec::new();
                            for byte in values {
                                if let Some(byte) = byte.as_i64() {
                                    ints.push(byte as i32);
                                } else {
                                    return Err(IOError::new(
                                        ErrorKind::InvalidData,
                                        "not a i32 in i32 array",
                                    ));
                                }
                            }
                            Self::IntArrayTag(ints)
                        }
                        12 => {
                            let mut longs = Vec::new();
                            for byte in values {
                                if let Some(byte) = byte.as_i64() {
                                    longs.push(byte);
                                } else {
                                    return Err(IOError::new(
                                        ErrorKind::InvalidData,
                                        "not a i64 in i64 array",
                                    ));
                                }
                            }
                            Self::LongArrayTag(longs)
                        }
                        _ => {
                            let mut all = Vec::new();
                            for value in values {
                                let nbt = Self::from_json(value)?;
                                match nbt {
                                    Self::CompoundTag(_) => {
                                        all.push(nbt);
                                    }
                                    _ => {
                                        let mut map = HashMap::new();
                                        map.insert("".to_string(), nbt);
                                        all.push(Self::CompoundTag(map));
                                    }
                                }
                            }
                            Self::ListTag(10, all)
                        }
                    }
                }
            }
            Value::Object(value) => {
                let mut map = HashMap::new();
                for (name, value) in value {
                    map.insert(name.to_owned(), NbtType::from_json(value)?);
                }
                Self::CompoundTag(map)
            }
            Value::Null => {
                return Err(IOError::new(ErrorKind::InvalidData, "null in json"));
            }
        })
    }

    pub fn to_json(&self) -> Value {
        match self {
            // Self::EndTag => Value::Null,
            Self::ByteTag(value) => Value::Number((*value).into()),
            Self::ShortTag(value) => Value::Number((*value).into()),
            Self::IntTag(value) => Value::Number((*value).into()),
            Self::LongTag(value) => Value::Number((*value).into()),
            Self::FloatTag(value) => {
                if value.is_normal() {
                    Value::Number(Number::from_f64(*value as f64).unwrap())
                } else if value.is_infinite() {
                    if value.is_sign_positive() {
                        Value::String("Infinity".to_string())
                    } else {
                        Value::String("-Infinity".to_string())
                    }
                } else if *value == -0.0 || *value == 0.0 {
                    Value::Number(Number::from_f64(*value as f64).unwrap())
                } else {
                    Value::String("NaN".to_string())
                }
            }
            Self::DoubleTag(value) => {
                if value.is_normal() {
                    Value::Number(Number::from_f64(*value).unwrap())
                } else if value.is_infinite() {
                    if value.is_sign_positive() {
                        Value::String("Infinity".to_string())
                    } else {
                        Value::String("-Infinity".to_string())
                    }
                } else if *value == -0.0 || *value == 0.0 {
                    Value::Number(Number::from_f64(*value).unwrap())
                } else {
                    Value::String("NaN".to_string())
                }
            }
            Self::ByteArrayTag(value) => {
                let mut data = Vec::new();
                for byte in value {
                    data.push(Value::Number((*byte).into()));
                }
                Value::Array(data)
            }
            Self::IntArrayTag(value) => {
                let mut data = Vec::new();
                for int in value {
                    data.push(Value::Number((*int).into()));
                }
                Value::Array(data)
            }
            Self::LongArrayTag(value) => {
                let mut data = Vec::new();
                for long in value {
                    data.push(Value::Number((*long).into()));
                }
                Value::Array(data)
            }
            Self::StringTag(value) => Value::String(value.to_owned()),
            Self::ListTag(_, values) => {
                let mut data = Vec::new();
                for nbt in values {
                    if let NbtType::CompoundTag(map) = nbt {
                        if map.len() == 1 {
                            let first = map.get("");
                            if let Some(first) = first {
                                data.push(first.to_json());
                                continue;
                            }
                        }
                    }
                    data.push(nbt.to_json());
                }
                Value::Array(data)
            }
            Self::CompoundTag(values) => {
                let mut map = Map::new();
                for (name, nbt) in values {
                    map.insert(name.to_owned(), nbt.to_json());
                }
                Value::Object(map)
            }
        }
    }

    pub fn id(&self) -> i8 {
        match self {
            // Self::EndTag => 0,
            Self::ByteTag(_) => 1,
            Self::ShortTag(_) => 2,
            Self::IntTag(_) => 3,
            Self::LongTag(_) => 4,
            Self::FloatTag(_) => 5,
            Self::DoubleTag(_) => 6,
            Self::ByteArrayTag(_) => 7,
            Self::StringTag(_) => 8,
            Self::ListTag(_, _) => 9,
            Self::CompoundTag(_) => 10,
            Self::IntArrayTag(_) => 11,
            Self::LongArrayTag(_) => 12,
        }
    }

    fn read<R: ReadBytesExt + ?Sized>(
        input: &mut R,
        counter: &mut NbtCounter,
        id: i8,
    ) -> IOResult<Self>
    where
        Self: Sized,
    {
        Ok(match id {
            //0 => Self::EndTag,
            1 => {
                counter.account_bytes(9)?;
                Self::ByteTag(input.read_i8()?)
            }
            2 => {
                counter.account_bytes(10)?;
                Self::ShortTag(input.read_i16::<BE>()?)
            }
            3 => {
                counter.account_bytes(12)?;
                Self::IntTag(input.read_i32::<BE>()?)
            }
            4 => {
                counter.account_bytes(14)?;
                Self::LongTag(input.read_i64::<BE>()?)
            }
            5 => {
                counter.account_bytes(12)?;
                Self::FloatTag(input.read_f32::<BE>()?)
            }
            6 => {
                counter.account_bytes(14)?;
                Self::DoubleTag(input.read_f64::<BE>()?)
            }
            7 => {
                counter.account_bytes(24)?;
                let len = input.read_i32::<BE>()?;
                if len < 0 {
                    return Err(IOError::new(ErrorKind::InvalidData, "negative array len"));
                }
                let len = len as usize;

                counter.account_bytes(len as u64)?;
                // this should be faster
                let mut byte_array = vec![0u8; len];
                input.read_exact(&mut byte_array)?;
                Self::ByteArrayTag(unsafe { core::mem::transmute(byte_array) })
            }
            8 => {
                counter.account_bytes(36)?;
                let string = read_java_utf(input)?;
                counter.account_bytes((string.len() * 2) as u64)?;
                Self::StringTag(string)
            }
            9 => {
                counter.push()?;
                counter.account_bytes(37)?;
                let nbt_type = input.read_i8()?;
                let amt = input.read_i32::<BE>()?;

                if nbt_type == 0 && amt > 0 {
                    return Err(IOError::new(
                        ErrorKind::InvalidData,
                        "Missing type on ListTag",
                    ));
                }
                let mut tags: Vec<NbtType> = Vec::new();

                counter.account_bytes((4 * amt) as u64)?;
                for _ in 0..amt {
                    tags.push(NbtType::read(input, counter, nbt_type)?);
                }
                counter.pop()?;
                Self::ListTag(nbt_type, tags)
            }
            10 => {
                counter.push()?;
                counter.account_bytes(48)?;
                let mut map: HashMap<String, NbtType> = HashMap::new();

                loop {
                    let t: i8 = input.read_i8()?;
                    if t == 0 {
                        break;
                    }
                    let string = read_java_utf(input)?;
                    counter.account_bytes(28)?;
                    counter.account_bytes((2 * string.len()) as u64)?;
                    let tag = NbtType::read(input, counter, t)?;
                    if map.insert(string, tag).is_none() {
                        counter.account_bytes(36)?;
                    }
                }
                counter.pop()?;
                Self::CompoundTag(map)
            }
            11 => {
                counter.account_bytes(24)?;
                let len = input.read_i32::<BE>()?;
                if len < 0 {
                    return Err(IOError::new(
                        ErrorKind::InvalidData,
                        format!("negative int arr size {len}"),
                    ));
                }
                counter.account_bytes((4 * len) as u64)?;

                let mut ints: Vec<i32> = vec![0; len as usize];

                for index in 0..len {
                    ints[index as usize] = input.read_i32::<BE>()?;
                }
                Self::IntArrayTag(ints)
            }
            12 => {
                counter.account_bytes(24)?;
                let len = input.read_i32::<BE>()?;
                if len < 0 {
                    return Err(IOError::new(
                        ErrorKind::InvalidData,
                        format!("negative int arr size {len}"),
                    ));
                }
                counter.account_bytes((8 * len) as u64)?;
                let mut longs: Vec<i64> = vec![0; len as usize];

                for index in 0..len {
                    longs[index as usize] = input.read_i64::<BE>()?;
                }
                Self::LongArrayTag(longs)
            }
            _ => {
                return Err(IOError::new(
                    ErrorKind::InvalidData,
                    format!("unknown nbt type {id}"),
                ));
            }
        })
    }

    fn write<W: WriteBytesExt + ?Sized>(&self, out: &mut W) -> IOResult<()> {
        match self {
            // Self::EndTag => {}
            Self::ByteTag(value) => {
                out.write_i8(*value)?;
            }
            Self::ShortTag(value) => {
                out.write_i16::<BE>(*value)?;
            }
            Self::IntTag(value) => {
                out.write_i32::<BE>(*value)?;
            }
            Self::LongTag(value) => {
                out.write_i64::<BE>(*value)?;
            }
            Self::FloatTag(value) => {
                out.write_f32::<BE>(*value)?;
            }
            Self::DoubleTag(value) => {
                out.write_f64::<BE>(*value)?;
            }
            Self::ByteArrayTag(value) => {
                out.write_i32::<BE>(value.len() as i32)?;
                out.write_all(unsafe { core::mem::transmute::<_, &Vec<u8>>(value) })?;
            }
            Self::StringTag(value) => {
                write_java_utf(out, value)?;
            }
            Self::ListTag(nbt_type, value) => {
                out.write_i8(*nbt_type)?;
                out.write_i32::<BE>(value.len() as i32)?;
                for tag in value {
                    NbtType::write(tag, out)?;
                }
            }
            Self::CompoundTag(value) => {
                for (string, nbt_type) in value {
                    let id = nbt_type.id();
                    out.write_i8(id)?;
                    if id != 0 {
                        write_java_utf(out, string)?;
                        NbtType::write(nbt_type, out)?;
                    }
                }
                out.write_i8(0)?;
            }
            Self::IntArrayTag(value) => {
                out.write_i32::<BE>(value.len() as i32)?;
                for int in value {
                    out.write_i32::<BE>(*int)?;
                }
            }
            Self::LongArrayTag(value) => {
                out.write_i32::<BE>(value.len() as i32)?;
                for long in value {
                    out.write_i64::<BE>(*long)?;
                }
            }
        }
        Ok(())
    }
}

pub fn read_networking_nbt<R: ReadBytesExt + ?Sized>(
    input: &mut R,
    version: i32,
) -> IOResult<Either<Option<NbtType>, NamedTag>> {
    let mut counter = NbtCounter {
        depth: 0,
        max_bytes: u64::MAX,
        used_bytes: 0,
    };
    let tag_type = input.read_i8()?;
    if tag_type == 0 {
        return Ok(Either::Left(None));
    }
    if version >= version::R1_20_2 {
        Ok(Either::Left(Some(NbtType::read(
            input,
            &mut counter,
            tag_type,
        )?)))
    } else {
        counter.account_bytes(28)?;
        let name = read_java_utf(input)?;
        counter.account_bytes((name.len() * 2) as u64)?;
        let tag = NbtType::read(input, &mut counter, tag_type)?;
        Ok(Either::Right(NamedTag { name, tag }))
    }
}

pub fn write_networking_nbt<W: WriteBytesExt + ?Sized>(
    out: &mut W,
    _: i32,
    either: &Either<Option<NbtType>, NamedTag>,
) -> IOResult<()> {
    if let Some(option) = either.as_ref().left() {
        if let Some(nbt) = option {
            out.write_i8(nbt.id())?;
            nbt.write(out)?;
        } else {
            out.write_i8(0)?;
        }
    } else {
        let named_tag = either.as_ref().right().unwrap();
        write_java_utf(out, &named_tag.name)?;
        named_tag.tag.write(out)?;
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq)]
pub struct NamedTag {
    name: String,
    tag: NbtType,
}

pub fn read_java_utf<R: ReadBytesExt + ?Sized>(input: &mut R) -> IOResult<String> {
    let utflen = input.read_u16::<BE>()?;
    let mut bytearr = vec![0; utflen as usize];
    input.read_exact(&mut bytearr)?;
    Ok(cesu8::from_java_cesu8(&bytearr)
        .map_err(|err| IOError::new(ErrorKind::InvalidData, err))?
        .to_string())
}

pub fn write_java_utf<W: WriteBytesExt + ?Sized>(out: &mut W, string: &str) -> IOResult<()> {
    let encoded = to_java_cesu8(string);
    out.write_u16::<BE>(encoded.len() as u16)?;
    out.write_all(&encoded)?;
    Ok(())
}

pub struct NbtCounter {
    used_bytes: u64,
    max_bytes: u64,
    depth: u16,
}

impl NbtCounter {
    pub fn account_bytes(&mut self, bytes: u64) -> IOResult<()> {
        if self.used_bytes + bytes > self.max_bytes {
            return Err(IOError::new(ErrorKind::InvalidData, "exeeded byte limit"));
        }
        self.used_bytes += bytes;
        Ok(())
    }

    pub fn push(&mut self) -> IOResult<()> {
        self.depth += 1;
        if self.depth > 512 {
            return Err(IOError::new(ErrorKind::InvalidData, "depth limit exeeded"));
        }
        Ok(())
    }

    pub fn pop(&mut self) -> IOResult<()> {
        if self.depth == 0 {
            return Err(IOError::new(
                ErrorKind::InvalidData,
                "popped more than pushed",
            ));
        }
        self.depth -= 1;
        Ok(())
    }
}
