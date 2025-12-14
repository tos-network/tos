use std::collections::HashMap;

use crate::crypto::Hash;
use crate::serializer::Serializer;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum ArgError {
    #[error("Invalid value for this argument type")]
    InvalidType,
    #[error("Argument '{}' not found", _0)]
    NotFound(String),
}

pub enum ArgValue {
    Bool(bool),
    Number(u64),
    String(String),
    Hash(Hash),
    Array(Vec<ArgValue>),
}

impl ArgValue {
    pub fn to_bool(self) -> Result<bool, ArgError> {
        match self {
            ArgValue::Bool(b) => Ok(b),
            _ => Err(ArgError::InvalidType),
        }
    }

    pub fn to_number(self) -> Result<u64, ArgError> {
        match self {
            ArgValue::Number(n) => Ok(n),
            _ => Err(ArgError::InvalidType),
        }
    }

    pub fn to_string_value(self) -> Result<String, ArgError> {
        match self {
            ArgValue::String(s) => Ok(s),
            _ => Err(ArgError::InvalidType),
        }
    }

    pub fn to_hash(self) -> Result<Hash, ArgError> {
        match self {
            ArgValue::Hash(hash) => Ok(hash),
            _ => Err(ArgError::InvalidType),
        }
    }

    pub fn to_vec(self) -> Result<Vec<ArgValue>, ArgError> {
        match self {
            ArgValue::Array(v) => Ok(v),
            _ => Err(ArgError::InvalidType),
        }
    }
}

pub enum ArgType {
    Bool,
    Number,
    String,
    Hash,
    Array(Box<ArgType>),
}

impl ArgType {
    pub fn to_value(&self, value: &str) -> Result<ArgValue, ArgError> {
        Ok(match self {
            ArgType::Bool => {
                let value = value.to_lowercase();
                if ["true", "yes", "y", "1"].contains(&value.as_str()) {
                    ArgValue::Bool(true)
                } else if ["false", "no", "n", "0"].contains(&value.as_str()) {
                    ArgValue::Bool(false)
                } else {
                    return Err(ArgError::InvalidType);
                }
            }
            ArgType::Number => ArgValue::Number(value.parse().map_err(|_| ArgError::InvalidType)?),
            ArgType::String => ArgValue::String(value.to_owned()),
            ArgType::Hash => {
                ArgValue::Hash(Hash::from_hex(value).map_err(|_| ArgError::InvalidType)?)
            }
            ArgType::Array(value_type) => {
                let values = value.split(",");
                let mut array: Vec<ArgValue> = Vec::new();
                for value in values {
                    let arg_value = value_type.to_value(value)?;
                    array.push(arg_value);
                }
                ArgValue::Array(array)
            }
        })
    }
}

pub struct Arg {
    name: String,
    arg_type: ArgType,
    description: String,
}

impl Arg {
    /// Create a new argument with name, type, and description
    pub fn new(name: &str, arg_type: ArgType, description: &str) -> Self {
        Self {
            name: name.to_owned(),
            arg_type,
            description: description.to_owned(),
        }
    }

    /// Create a new argument without description (for backward compatibility)
    #[allow(dead_code)]
    pub fn new_simple(name: &str, arg_type: ArgType) -> Self {
        Self {
            name: name.to_owned(),
            arg_type,
            description: String::new(),
        }
    }

    pub fn get_name(&self) -> &String {
        &self.name
    }

    pub fn get_type(&self) -> &ArgType {
        &self.arg_type
    }

    pub fn get_description(&self) -> &String {
        &self.description
    }
}

pub struct ArgumentManager {
    arguments: HashMap<String, ArgValue>,
}

impl ArgumentManager {
    pub fn new(arguments: HashMap<String, ArgValue>) -> Self {
        Self { arguments }
    }

    pub fn get_arguments(&self) -> &HashMap<String, ArgValue> {
        &self.arguments
    }

    pub fn get_value(&mut self, name: &str) -> Result<ArgValue, ArgError> {
        self.arguments
            .remove(name)
            .ok_or_else(|| ArgError::NotFound(name.to_owned()))
    }

    pub fn has_argument(&self, name: &str) -> bool {
        self.arguments.contains_key(name)
    }

    // Get flag value
    // If its not present, return false
    pub fn get_flag(&mut self, name: &str) -> Result<bool, ArgError> {
        self.arguments
            .remove(name)
            .map(|value| value.to_bool())
            .unwrap_or(Ok(false))
    }

    pub fn size(&self) -> usize {
        self.arguments.len()
    }

    /// Create ArgumentManager from JSON parameters
    pub fn from_json_params(
        params: &std::collections::HashMap<String, serde_json::Value>,
    ) -> Result<Self, ArgError> {
        let mut arguments = HashMap::new();

        for (key, value) in params {
            let arg_value = match value {
                serde_json::Value::Bool(b) => ArgValue::Bool(*b),
                serde_json::Value::Number(n) => {
                    if let Some(i) = n.as_u64() {
                        ArgValue::Number(i)
                    } else {
                        return Err(ArgError::InvalidType);
                    }
                }
                serde_json::Value::String(s) => {
                    // Try to parse as Hash first, then as String
                    if s.len() == 64 && s.chars().all(|c| c.is_ascii_hexdigit()) {
                        if let Ok(hash) = Hash::from_hex(s) {
                            ArgValue::Hash(hash)
                        } else {
                            ArgValue::String(s.clone())
                        }
                    } else {
                        ArgValue::String(s.clone())
                    }
                }
                serde_json::Value::Array(arr) => {
                    let mut vec_args = Vec::new();
                    for item in arr {
                        match item {
                            serde_json::Value::String(s) => {
                                vec_args.push(ArgValue::String(s.clone()))
                            }
                            serde_json::Value::Number(n) => {
                                if let Some(i) = n.as_u64() {
                                    vec_args.push(ArgValue::Number(i));
                                } else {
                                    return Err(ArgError::InvalidType);
                                }
                            }
                            serde_json::Value::Bool(b) => vec_args.push(ArgValue::Bool(*b)),
                            _ => return Err(ArgError::InvalidType),
                        }
                    }
                    ArgValue::Array(vec_args)
                }
                _ => return Err(ArgError::InvalidType),
            };

            arguments.insert(key.clone(), arg_value);
        }

        Ok(ArgumentManager::new(arguments))
    }
}
