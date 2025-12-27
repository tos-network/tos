use tos_common::{block::TopoHeight, serializer::*};

pub type AccountId = u64;

pub struct Account {
    // id used to prevent duplicated raw key
    // and save some space
    pub id: AccountId,
    // At which topoheight the account has been seen
    // for the first time
    pub registered_at: Option<TopoHeight>,
    // pointer to the last versioned nonce
    pub nonce_pointer: Option<TopoHeight>,
    // pointer to the last versioned multisig
    pub multisig_pointer: Option<TopoHeight>,
    // pointer to the last versioned energy resource
    pub energy_pointer: Option<TopoHeight>,
}

impl Serializer for Account {
    fn read(reader: &mut Reader) -> Result<Self, ReaderError> {
        let id = AccountId::read(reader)?;
        let registered_at = Option::read(reader)?;
        let nonce_pointer = Option::read(reader)?;
        let multisig_pointer = Option::read(reader)?;
        let energy_pointer = Option::read(reader)?;

        Ok(Self {
            id,
            registered_at,
            nonce_pointer,
            multisig_pointer,
            energy_pointer,
        })
    }

    fn write(&self, writer: &mut Writer) {
        self.id.write(writer);
        self.registered_at.write(writer);
        self.nonce_pointer.write(writer);
        self.multisig_pointer.write(writer);
        self.energy_pointer.write(writer);
    }

    fn size(&self) -> usize {
        self.id.size()
            + self.registered_at.size()
            + self.nonce_pointer.size()
            + self.multisig_pointer.size()
            + self.energy_pointer.size()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_account_with_energy_pointer_serialization() {
        let account = Account {
            id: 42,
            registered_at: Some(100),
            nonce_pointer: Some(200),
            multisig_pointer: Some(300),
            energy_pointer: Some(400),
        };

        // Serialize
        let mut bytes = Vec::new();
        let mut writer = Writer::new(&mut bytes);
        account.write(&mut writer);
        let bytes = writer.as_bytes().to_vec();

        // Deserialize
        let mut reader = Reader::new(&bytes);
        let decoded = Account::read(&mut reader).expect("Failed to deserialize Account");

        assert_eq!(decoded.id, 42);
        assert_eq!(decoded.registered_at, Some(100));
        assert_eq!(decoded.nonce_pointer, Some(200));
        assert_eq!(decoded.multisig_pointer, Some(300));
        assert_eq!(decoded.energy_pointer, Some(400));
    }

    #[test]
    fn test_account_with_none_energy_pointer() {
        let account = Account {
            id: 1,
            registered_at: Some(50),
            nonce_pointer: Some(100),
            multisig_pointer: None,
            energy_pointer: None,
        };

        // Serialize
        let mut bytes = Vec::new();
        let mut writer = Writer::new(&mut bytes);
        account.write(&mut writer);
        let bytes = writer.as_bytes().to_vec();

        // Deserialize
        let mut reader = Reader::new(&bytes);
        let decoded = Account::read(&mut reader).expect("Failed to deserialize Account");

        assert_eq!(decoded.id, 1);
        assert_eq!(decoded.registered_at, Some(50));
        assert_eq!(decoded.nonce_pointer, Some(100));
        assert_eq!(decoded.multisig_pointer, None);
        assert_eq!(decoded.energy_pointer, None);
    }

    #[test]
    fn test_account_size_includes_energy_pointer() {
        let account_with_energy = Account {
            id: 1,
            registered_at: Some(100),
            nonce_pointer: Some(200),
            multisig_pointer: Some(300),
            energy_pointer: Some(400),
        };

        let account_without_energy = Account {
            id: 1,
            registered_at: Some(100),
            nonce_pointer: Some(200),
            multisig_pointer: Some(300),
            energy_pointer: None,
        };

        // Account with Some(energy_pointer) should be larger than with None
        // Option<u64> with Some(value) = 1 byte (tag) + 8 bytes (u64)
        // Option<u64> with None = 1 byte (tag)
        let size_with = account_with_energy.size();
        let size_without = account_without_energy.size();

        assert!(
            size_with > size_without,
            "Account with energy_pointer should be larger: {} vs {}",
            size_with,
            size_without
        );

        // The difference should be 8 bytes (the u64 value)
        assert_eq!(
            size_with - size_without,
            8,
            "Size difference should be 8 bytes for u64"
        );
    }

    // Edge case tests (5.2.5)

    #[test]
    fn test_energy_pointer_with_very_large_topoheight() {
        let account = Account {
            id: u64::MAX,
            registered_at: Some(u64::MAX),
            nonce_pointer: Some(u64::MAX),
            multisig_pointer: Some(u64::MAX),
            energy_pointer: Some(u64::MAX),
        };

        // Serialize
        let mut bytes = Vec::new();
        let mut writer = Writer::new(&mut bytes);
        account.write(&mut writer);
        let bytes = writer.as_bytes().to_vec();

        // Deserialize
        let mut reader = Reader::new(&bytes);
        let decoded = Account::read(&mut reader).expect("Failed to deserialize Account");

        assert_eq!(decoded.id, u64::MAX);
        assert_eq!(decoded.registered_at, Some(u64::MAX));
        assert_eq!(decoded.nonce_pointer, Some(u64::MAX));
        assert_eq!(decoded.multisig_pointer, Some(u64::MAX));
        assert_eq!(decoded.energy_pointer, Some(u64::MAX));
    }

    #[test]
    fn test_multiple_accounts_with_energy_pointers() {
        // Test that multiple accounts serialize/deserialize independently
        let accounts = vec![
            Account {
                id: 1,
                registered_at: Some(100),
                nonce_pointer: Some(200),
                multisig_pointer: None,
                energy_pointer: Some(300),
            },
            Account {
                id: 2,
                registered_at: Some(150),
                nonce_pointer: None,
                multisig_pointer: Some(250),
                energy_pointer: None,
            },
            Account {
                id: 3,
                registered_at: None,
                nonce_pointer: Some(350),
                multisig_pointer: Some(400),
                energy_pointer: Some(450),
            },
        ];

        for (i, original) in accounts.iter().enumerate() {
            let mut bytes = Vec::new();
            let mut writer = Writer::new(&mut bytes);
            original.write(&mut writer);
            let bytes = writer.as_bytes().to_vec();

            let mut reader = Reader::new(&bytes);
            let decoded = Account::read(&mut reader).expect("Failed to deserialize");

            assert_eq!(decoded.id, original.id, "Account {} id mismatch", i);
            assert_eq!(
                decoded.registered_at, original.registered_at,
                "Account {} registered_at mismatch",
                i
            );
            assert_eq!(
                decoded.nonce_pointer, original.nonce_pointer,
                "Account {} nonce_pointer mismatch",
                i
            );
            assert_eq!(
                decoded.multisig_pointer, original.multisig_pointer,
                "Account {} multisig_pointer mismatch",
                i
            );
            assert_eq!(
                decoded.energy_pointer, original.energy_pointer,
                "Account {} energy_pointer mismatch",
                i
            );
        }
    }

    #[test]
    fn test_energy_pointer_persistence_across_serialization_cycles() {
        let original = Account {
            id: 12345,
            registered_at: Some(1000),
            nonce_pointer: Some(2000),
            multisig_pointer: Some(3000),
            energy_pointer: Some(4000),
        };

        // First cycle
        let mut bytes1 = Vec::new();
        let mut writer1 = Writer::new(&mut bytes1);
        original.write(&mut writer1);
        let bytes1 = writer1.as_bytes().to_vec();

        let mut reader1 = Reader::new(&bytes1);
        let decoded1 = Account::read(&mut reader1).expect("Cycle 1 failed");

        // Second cycle
        let mut bytes2 = Vec::new();
        let mut writer2 = Writer::new(&mut bytes2);
        decoded1.write(&mut writer2);
        let bytes2 = writer2.as_bytes().to_vec();

        let mut reader2 = Reader::new(&bytes2);
        let decoded2 = Account::read(&mut reader2).expect("Cycle 2 failed");

        // Third cycle
        let mut bytes3 = Vec::new();
        let mut writer3 = Writer::new(&mut bytes3);
        decoded2.write(&mut writer3);
        let bytes3 = writer3.as_bytes().to_vec();

        let mut reader3 = Reader::new(&bytes3);
        let decoded3 = Account::read(&mut reader3).expect("Cycle 3 failed");

        // All cycles should produce identical results
        assert_eq!(bytes1, bytes2, "Bytes differ between cycle 1 and 2");
        assert_eq!(bytes2, bytes3, "Bytes differ between cycle 2 and 3");

        assert_eq!(decoded3.id, original.id);
        assert_eq!(decoded3.registered_at, original.registered_at);
        assert_eq!(decoded3.nonce_pointer, original.nonce_pointer);
        assert_eq!(decoded3.multisig_pointer, original.multisig_pointer);
        assert_eq!(decoded3.energy_pointer, original.energy_pointer);
    }

    #[test]
    fn test_account_fields_independence() {
        // Verify that each field is independent - changing one doesn't affect others
        let base = Account {
            id: 100,
            registered_at: Some(1000),
            nonce_pointer: Some(2000),
            multisig_pointer: Some(3000),
            energy_pointer: Some(4000),
        };

        // Only change energy_pointer
        let modified = Account {
            id: 100,
            registered_at: Some(1000),
            nonce_pointer: Some(2000),
            multisig_pointer: Some(3000),
            energy_pointer: Some(9999), // Changed
        };

        let mut base_bytes = Vec::new();
        let mut base_writer = Writer::new(&mut base_bytes);
        base.write(&mut base_writer);
        let base_bytes = base_writer.as_bytes().to_vec();

        let mut mod_bytes = Vec::new();
        let mut mod_writer = Writer::new(&mut mod_bytes);
        modified.write(&mut mod_writer);
        let mod_bytes = mod_writer.as_bytes().to_vec();

        // Bytes should differ (because energy_pointer changed)
        assert_ne!(
            base_bytes, mod_bytes,
            "Bytes should differ when energy_pointer changes"
        );

        // But the first part should be identical (id, registered_at, nonce_pointer, multisig_pointer)
        // id: 8 bytes, registered_at: 1+8=9 bytes, nonce_pointer: 1+8=9 bytes, multisig_pointer: 1+8=9 bytes
        // Total prefix: 8+9+9+9 = 35 bytes
        let prefix_len = 35;
        assert_eq!(
            &base_bytes[..prefix_len],
            &mod_bytes[..prefix_len],
            "Prefix should be identical when only energy_pointer changes"
        );

        // Energy pointer part should differ
        assert_ne!(
            &base_bytes[prefix_len..],
            &mod_bytes[prefix_len..],
            "Suffix should differ when energy_pointer changes"
        );
    }
}
