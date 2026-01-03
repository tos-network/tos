mod deploy;
mod invoke;

use crate::serializer::*;
use anyhow::Context;
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use tos_kernel::Module;
use tos_kernel::{OpaqueWrapper, Primitive, ValueCell, U256};

pub use deploy::*;
pub use invoke::*;

/// Maximum nesting depth for ValueCell structures to prevent stack overflow DoS attacks
/// Example attack: Object([Object([Object([... 10000 levels ...])])])
const MAX_VALUE_CELL_DEPTH: usize = 64;

/// Maximum array size for ValueCell::Object to prevent memory exhaustion DoS attacks
/// Example attack: Object with 10M elements → gigabytes of memory allocation
const MAX_ARRAY_SIZE: usize = 10000;

/// Maximum map size for ValueCell::Map to prevent memory exhaustion DoS attacks
/// Example attack: Map with 10M key-value pairs → gigabytes of memory allocation
const MAX_MAP_SIZE: usize = 10000;

/// Maximum bytes size for ValueCell::Bytes to prevent memory exhaustion DoS attacks
/// Example attack: Bytes with 1GB length prefix → gigabytes of memory allocation
const MAX_BYTES_SIZE: usize = 1_000_000; // 1MB max

/// Contract deposit - plaintext balance system
///
/// Balance simplification: Only public deposits are supported.
/// The amount is plaintext and visible to everyone.
/// Private/encrypted deposits have been removed.
#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(transparent)]
pub struct ContractDeposit(pub u64);

impl ContractDeposit {
    /// Create a new contract deposit with the specified amount
    pub fn new(amount: u64) -> Self {
        Self(amount)
    }

    /// Get the deposit amount
    pub fn amount(&self) -> u64 {
        self.0
    }

    /// Extract the deposit amount (for backward compatibility)
    pub fn get_amount(&self) -> Result<u64, &'static str> {
        Ok(self.0)
    }
}

impl Serializer for ContractDeposit {
    fn write(&self, writer: &mut Writer) {
        // Write type tag (0 = Public) for potential future compatibility
        writer.write_u8(0);
        writer.write_u64(&self.0);
    }

    fn read(reader: &mut Reader) -> Result<ContractDeposit, ReaderError> {
        let type_tag = reader.read_u8()?;
        match type_tag {
            0 => {
                // Public deposit
                let amount = reader.read_u64()?;
                Ok(ContractDeposit(amount))
            }
            1 => {
                // Private deposits are no longer supported in development stage
                Err(ReaderError::InvalidValue)
            }
            _ => Err(ReaderError::InvalidValue),
        }
    }

    fn size(&self) -> usize {
        1 + 8 // type tag (1 byte) + u64 amount (8 bytes)
    }
}

impl Serializer for U256 {
    fn write(&self, writer: &mut Writer) {
        writer.write_bytes(&self.to_be_bytes());
    }

    fn read(reader: &mut Reader) -> Result<U256, ReaderError> {
        Ok(U256::from_be_bytes(reader.read_bytes(32)?))
    }

    fn size(&self) -> usize {
        32
    }
}

impl Serializer for Primitive {
    fn write(&self, writer: &mut Writer) {
        match self {
            Primitive::Null => writer.write_u8(0),
            Primitive::U8(value) => {
                writer.write_u8(1);
                writer.write_u8(*value);
            }
            Primitive::U16(value) => {
                writer.write_u8(2);
                writer.write_u16(*value);
            }
            Primitive::U32(value) => {
                writer.write_u8(3);
                writer.write_u32(value);
            }
            Primitive::U64(value) => {
                writer.write_u8(4);
                writer.write_u64(value);
            }
            Primitive::U128(value) => {
                writer.write_u8(5);
                writer.write_u128(value);
            }
            Primitive::U256(value) => {
                writer.write_u8(6);
                value.write(writer);
            }
            Primitive::Boolean(value) => {
                writer.write_u8(7);
                writer.write_bool(*value);
            }
            Primitive::String(value) => {
                writer.write_u8(8);
                let bytes = value.as_bytes();
                writer.write_u16(bytes.len() as u16);
                writer.write_bytes(bytes);
            }
            Primitive::Range(range) => {
                writer.write_u8(9);
                range.0.write(writer);
                range.1.write(writer);
            }
            Primitive::Opaque(opaque) => {
                writer.write_u8(10);
                opaque.write(writer);
            }
        }
    }

    fn read(reader: &mut Reader) -> Result<Primitive, ReaderError> {
        Ok(match reader.read_u8()? {
            0 => Primitive::Null,
            1 => Primitive::U8(reader.read_u8()?),
            2 => Primitive::U16(reader.read_u16()?),
            3 => Primitive::U32(reader.read_u32()?),
            4 => Primitive::U64(reader.read_u64()?),
            5 => Primitive::U128(reader.read_u128()?),
            6 => Primitive::U256(U256::read(reader)?),
            7 => Primitive::Boolean(reader.read_bool()?),
            8 => {
                let len = reader.read_u16()? as usize;
                Primitive::String(reader.read_string_with_size(len)?)
            }
            9 => {
                let left = Primitive::read(reader)?;
                if !left.is_number() {
                    return Err(ReaderError::InvalidValue);
                }

                let right = Primitive::read(reader)?;
                if !right.is_number() {
                    return Err(ReaderError::InvalidValue);
                }

                let left_type = left.get_type().context("left range type")?;
                let right_type = right.get_type().context("right range type")?;
                if left_type != right_type {
                    return Err(ReaderError::InvalidValue);
                }

                Primitive::Range(Box::new((left, right)))
            }
            10 => Primitive::Opaque(OpaqueWrapper::read(reader)?),
            _ => return Err(ReaderError::InvalidValue),
        })
    }

    fn size(&self) -> usize {
        1 + match self {
            Primitive::Null => 0,
            Primitive::U8(_) => 1,
            Primitive::U16(_) => 2,
            Primitive::U32(_) => 4,
            Primitive::U64(_) => 8,
            Primitive::U128(_) => 16,
            Primitive::U256(value) => value.size(),
            Primitive::Boolean(_) => 1,
            Primitive::String(value) => 2 + value.len(),
            Primitive::Range(range) => range.0.size() + range.1.size(),
            Primitive::Opaque(opaque) => opaque.size(),
        }
    }
}

/// Helper enum for iterative ValueCell deserialization
/// Tracks the state of containers being built during deserialization
enum BuildState {
    /// Building an Object array, need to read `remaining` more ValueCells
    Object {
        values: Vec<ValueCell>,
        remaining: usize,
    },
    /// Building a Map, need to read `remaining` more key-value pairs
    /// `pending_key` holds a key waiting for its value to be read
    Map {
        map: IndexMap<ValueCell, ValueCell>,
        remaining: usize,
        pending_key: Option<ValueCell>,
    },
}

impl Serializer for ValueCell {
    // Serialize a value cell
    // ValueCell with more than one value are serialized in reverse order
    // This help us to save a reverse operation when deserializing
    fn write(&self, writer: &mut Writer) {
        match self {
            ValueCell::Default(value) => {
                writer.write_u8(0);
                value.write(writer);
            }
            ValueCell::Bytes(bytes) => {
                writer.write_u8(1);
                let len = bytes.len() as u32;
                writer.write_u32(&len);
                writer.write_bytes(bytes);
            }
            ValueCell::Object(values) => {
                writer.write_u8(2);
                let len = values.len() as u32;
                writer.write_u32(&len);
                for value in values.iter() {
                    value.write(writer);
                }
            }
            ValueCell::Map(map) => {
                writer.write_u8(3);
                let len = map.len() as u32;
                writer.write_u32(&len);
                for (key, value) in map.iter() {
                    key.write(writer);
                    value.write(writer);
                }
            }
        };
    }

    /// Iterative deserialization with depth and size limits to prevent DoS attacks
    ///
    /// SECURITY FIX (CVE-TOS-2025-001): Replaced recursive implementation with iterative
    /// to prevent stack overflow attacks via deeply nested ValueCell structures.
    ///
    /// Enforces limits:
    /// - MAX_VALUE_CELL_DEPTH (64): Maximum nesting depth
    /// - MAX_ARRAY_SIZE (10000): Maximum Object array size
    /// - MAX_MAP_SIZE (10000): Maximum Map size
    fn read(reader: &mut Reader) -> Result<ValueCell, ReaderError> {
        // Stack of containers being built (Objects and Maps)
        let mut build_stack: Vec<BuildState> = Vec::new();

        // Current nesting depth (incremented when entering Object/Map, decremented when exiting)
        let mut depth = 0usize;

        // The most recently completed value (None initially, waiting to read first value)
        let mut current_value: Option<ValueCell> = None;

        loop {
            // Check depth limit before reading
            if depth > MAX_VALUE_CELL_DEPTH {
                return Err(ReaderError::ExceedsMaxDepth {
                    max: MAX_VALUE_CELL_DEPTH,
                    actual: depth,
                });
            }

            // If we have a completed value, try to incorporate it into parent container
            if let Some(value) = current_value.take() {
                match build_stack.pop() {
                    None => {
                        // No parent container → this is the final result
                        return Ok(value);
                    }
                    Some(BuildState::Object {
                        mut values,
                        remaining,
                    }) => {
                        // Add value to Object array
                        values.push(value);

                        if remaining == 1 {
                            // Object is now complete
                            current_value = Some(ValueCell::Object(values));
                            depth -= 1;
                            // Loop back to State 2 (have completed value)
                            continue;
                        } else {
                            // Need to read more items for this Object
                            build_stack.push(BuildState::Object {
                                values,
                                remaining: remaining - 1,
                            });
                            // Fall through to State 1 (read next value)
                        }
                    }
                    Some(BuildState::Map {
                        mut map,
                        remaining,
                        pending_key,
                    }) => {
                        match pending_key {
                            None => {
                                // This value is a key, now wait for its value
                                build_stack.push(BuildState::Map {
                                    map,
                                    remaining,
                                    pending_key: Some(value),
                                });
                                // Fall through to State 1 (read the value part)
                            }
                            Some(key) => {
                                // This value completes a key-value pair
                                map.insert(key, value);

                                if remaining == 1 {
                                    // Map is now complete
                                    current_value = Some(ValueCell::Map(Box::new(map)));
                                    depth -= 1;
                                    // Loop back to State 2 (have completed value)
                                    continue;
                                } else {
                                    // Need to read more key-value pairs
                                    build_stack.push(BuildState::Map {
                                        map,
                                        remaining: remaining - 1,
                                        pending_key: None,
                                    });
                                    // Fall through to State 1 (read next key)
                                }
                            }
                        }
                    }
                }
            }

            // State 1: Read the next ValueCell
            match reader.read_u8()? {
                0 => {
                    // Default: Primitive value
                    current_value = Some(ValueCell::Default(Primitive::read(reader)?));
                    // Loop back to State 2 (have completed value)
                }
                1 => {
                    // Bytes: Read length and byte array
                    let len = reader.read_u32()? as usize;

                    // Enforce bytes size limit to prevent memory exhaustion DoS
                    if len > MAX_BYTES_SIZE {
                        return Err(ReaderError::ExceedsMaxBytesSize {
                            max: MAX_BYTES_SIZE,
                            actual: len,
                        });
                    }

                    current_value = Some(ValueCell::Bytes(reader.read_bytes(len)?));
                    // Loop back to State 2 (have completed value)
                }
                2 => {
                    // Object: Read array of ValueCells
                    let len = reader.read_u32()? as usize;

                    // Enforce array size limit
                    if len > MAX_ARRAY_SIZE {
                        return Err(ReaderError::ExceedsMaxArraySize {
                            max: MAX_ARRAY_SIZE,
                            actual: len,
                        });
                    }

                    if len == 0 {
                        // Empty Object
                        current_value = Some(ValueCell::Object(Vec::new()));
                        // Loop back to State 2 (have completed value)
                    } else {
                        // Push work to read `len` ValueCells
                        depth += 1;
                        build_stack.push(BuildState::Object {
                            values: Vec::with_capacity(len),
                            remaining: len,
                        });
                        // Loop back to State 1 (read first element)
                    }
                }
                3 => {
                    // Map: Read key-value pairs
                    let len = reader.read_u32()? as usize;

                    // Enforce map size limit
                    if len > MAX_MAP_SIZE {
                        return Err(ReaderError::ExceedsMaxMapSize {
                            max: MAX_MAP_SIZE,
                            actual: len,
                        });
                    }

                    if len == 0 {
                        // Empty Map
                        current_value = Some(ValueCell::Map(Box::new(IndexMap::new())));
                        // Loop back to State 2 (have completed value)
                    } else {
                        // Push work to read `len` key-value pairs
                        depth += 1;
                        build_stack.push(BuildState::Map {
                            map: IndexMap::with_capacity(len),
                            remaining: len,
                            pending_key: None,
                        });
                        // Loop back to State 1 (read first key)
                    }
                }
                _ => return Err(ReaderError::InvalidValue),
            }
        }
    }

    fn size(&self) -> usize {
        let mut total = 0;
        let mut stack = vec![self];

        while let Some(cell) = stack.pop() {
            // variant id
            total += 1;
            match cell {
                ValueCell::Default(value) => total += value.size(),
                ValueCell::Bytes(bytes) => {
                    // u32 len
                    total += 4;
                    total += bytes.len();
                }
                ValueCell::Object(values) => {
                    // u32 len
                    total += 4;
                    for value in values {
                        stack.push(value);
                    }
                }
                ValueCell::Map(map) => {
                    // u32 len
                    total += 4;
                    for (key, value) in map.iter() {
                        stack.push(value);
                        stack.push(key);
                    }
                }
            }
        }

        total
    }
}

impl Serializer for Module {
    fn write(&self, writer: &mut Writer) {
        // Simplified format: bytecode length (u32) + bytecode
        let bytecode = self.get_bytecode().unwrap_or(&[]);
        writer.write_u32(&(bytecode.len() as u32));
        writer.write_bytes(bytecode);
    }

    fn read(reader: &mut Reader) -> Result<Module, ReaderError> {
        let bytecode_len = reader.read_u32()? as usize;

        // Sanity check: max 10MB for contract bytecode
        if bytecode_len > 10 * 1024 * 1024 {
            return Err(ReaderError::InvalidSize);
        }

        let bytecode: Vec<u8> = reader.read_bytes(bytecode_len)?;

        // Verify ELF magic bytes
        if bytecode.len() < 4 || &bytecode[0..4] != b"\x7FELF" {
            return Err(ReaderError::InvalidValue);
        }

        Ok(Module::from_bytecode(bytecode))
    }
}

#[cfg(test)]
#[allow(clippy::disallowed_methods)]
mod tests {
    use super::*;
    use crate::crypto::Hash;

    #[test]
    fn test_serde_module() {
        // Create a mock ELF bytecode (starts with ELF magic)
        let mock_elf = vec![
            0x7F, b'E', b'L', b'F', // ELF magic
            0x02, // 64-bit
            0x01, // Little endian
            0x01, // ELF version
            0x00, // OS/ABI
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // Padding
            0x03, 0x00, // Type: shared object (for .so files)
            0xF7, 0x00, // Machine: BPF
            // ... more bytes to make it look like a valid ELF
            0x01, 0x00, 0x00, 0x00, // Version
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // Entry point
            0x40, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // Program header offset
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // Section header offset
            0x00, 0x00, 0x00, 0x00, // Flags
            0x40, 0x00, // ELF header size
            0x38, 0x00, // Program header entry size
            0x01, 0x00, // Program header count
            0x40, 0x00, // Section header entry size
            0x00, 0x00, // Section header count
            0x00, 0x00, // Section name string table index
        ];

        // Create a module from bytecode
        let module = Module::from_bytecode(mock_elf.clone());
        assert!(module.has_bytecode());
        assert_eq!(module.get_bytecode(), Some(mock_elf.as_slice()));

        // Serialize and deserialize
        let bytes = module.to_bytes();
        let deserialized = Module::from_bytes(&bytes).expect("deserialization should succeed");

        // Verify deserialized module
        assert!(deserialized.has_bytecode());
        assert_eq!(deserialized.get_bytecode(), Some(mock_elf.as_slice()));

        // Verify size calculation
        // Simplified format: length (4) + bytecode
        assert_eq!(bytes.len(), 4 + mock_elf.len());
    }

    #[track_caller]
    fn test_serde_cell(cell: ValueCell) {
        let bytes = cell.to_bytes();
        let v = ValueCell::from_bytes(&bytes).expect("deserialization should succeed");

        assert_eq!(v, cell);
    }

    #[test]
    fn test_serde_primitive() {
        test_serde_cell(ValueCell::Default(Primitive::Null));
        test_serde_cell(ValueCell::Default(Primitive::Boolean(false)));
        test_serde_cell(ValueCell::Default(Primitive::U8(42)));
        test_serde_cell(ValueCell::Default(Primitive::U32(42)));
        test_serde_cell(ValueCell::Default(Primitive::U64(42)));
        test_serde_cell(ValueCell::Default(Primitive::U128(42)));
        test_serde_cell(ValueCell::Default(Primitive::U256(42u64.into())));
        test_serde_cell(ValueCell::Default(Primitive::Range(Box::new((
            Primitive::U128(42),
            Primitive::U128(420),
        )))));
        test_serde_cell(ValueCell::Default(Primitive::String(
            "hello world!!!".to_owned(),
        )));

        test_serde_cell(ValueCell::Default(Primitive::Opaque(OpaqueWrapper::new(
            Hash::zero(),
        ))));
    }

    #[test]
    fn test_serde_value_cell() {
        test_serde_cell(ValueCell::Bytes(vec![0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10]));
        test_serde_cell(ValueCell::Object(vec![
            ValueCell::Default(Primitive::U64(42)),
            ValueCell::Default(Primitive::U64(42)),
            ValueCell::Default(Primitive::U64(42)),
            ValueCell::Default(Primitive::U64(42)),
            ValueCell::Default(Primitive::U64(42)),
        ]));
        test_serde_cell(ValueCell::Map(Box::new(
            [(
                ValueCell::Default(Primitive::U64(42)),
                ValueCell::Default(Primitive::String("Hello World!".to_owned())),
            )]
            .into_iter()
            .collect(),
        )));
    }

    /// Test that depth limit is enforced to prevent stack overflow DoS
    /// CVE-TOS-2025-001: Security fix for recursive deserialization
    #[test]
    fn test_value_cell_depth_limit() {
        // Create a deeply nested structure: Object([Object([Object([...])])])
        // Depth exceeds MAX_VALUE_CELL_DEPTH (64 levels)
        let mut buffer = Vec::new();
        let mut writer = Writer::new(&mut buffer);

        // Write 65 nested Objects (exceeds limit of 64)
        for _ in 0..65 {
            writer.write_u8(2); // Object tag
            writer.write_u32(&1); // len = 1 (single nested element)
        }
        // Write innermost value (primitive)
        writer.write_u8(0); // Default tag
        writer.write_u8(0); // Primitive::Null tag

        let bytes = writer.as_bytes();
        let mut reader = Reader::new(bytes);

        // Should fail with ExceedsMaxDepth error
        let result = ValueCell::read(&mut reader);
        assert!(result.is_err());
        match result {
            Err(ReaderError::ExceedsMaxDepth { max, actual }) => {
                assert_eq!(max, MAX_VALUE_CELL_DEPTH);
                assert_eq!(actual, 65);
            }
            _ => panic!("Expected ExceedsMaxDepth error, got {result:?}"),
        }
    }

    /// Test that array size limit is enforced to prevent memory exhaustion DoS
    #[test]
    fn test_value_cell_array_size_limit() {
        // Create Object with size exceeding MAX_ARRAY_SIZE (10000 elements)
        let mut buffer = Vec::new();
        let mut writer = Writer::new(&mut buffer);
        writer.write_u8(2); // Object tag
        writer.write_u32(&10001); // len = 10001 (exceeds limit)

        let bytes = writer.as_bytes();
        let mut reader = Reader::new(bytes);

        // Should fail with ExceedsMaxArraySize error
        let result = ValueCell::read(&mut reader);
        assert!(result.is_err());
        match result {
            Err(ReaderError::ExceedsMaxArraySize { max, actual }) => {
                assert_eq!(max, MAX_ARRAY_SIZE);
                assert_eq!(actual, 10001);
            }
            _ => panic!("Expected ExceedsMaxArraySize error, got {result:?}"),
        }
    }
}
