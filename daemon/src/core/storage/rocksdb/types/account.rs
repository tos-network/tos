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
}

impl Serializer for Account {
    fn read(reader: &mut Reader) -> Result<Self, ReaderError> {
        Ok(Self {
            id: AccountId::read(reader)?,
            registered_at: Option::read(reader)?,
            nonce_pointer: Option::read(reader)?,
            multisig_pointer: Option::read(reader)?,
        })
    }

    fn write(&self, writer: &mut Writer) {
        self.id.write(writer);
        self.registered_at.write(writer);
        self.nonce_pointer.write(writer);
        self.multisig_pointer.write(writer);
    }

    fn size(&self) -> usize {
        self.id.size()
            + self.registered_at.size()
            + self.nonce_pointer.size()
            + self.multisig_pointer.size()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_account_roundtrip() {
        let account = Account {
            id: 42,
            registered_at: Some(100),
            nonce_pointer: Some(200),
            multisig_pointer: Some(300),
        };

        let mut bytes = Vec::new();
        let mut writer = Writer::new(&mut bytes);
        account.write(&mut writer);

        let mut reader = Reader::new(writer.as_bytes());
        let decoded = Account::read(&mut reader).expect("deserialize account");

        assert_eq!(decoded.id, 42);
        assert_eq!(decoded.registered_at, Some(100));
        assert_eq!(decoded.nonce_pointer, Some(200));
        assert_eq!(decoded.multisig_pointer, Some(300));
    }
}
