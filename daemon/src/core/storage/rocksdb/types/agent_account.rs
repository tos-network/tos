use tos_common::serializer::{Reader, ReaderError, Serializer, Writer};

pub type AgentSessionKeyId = u64;

pub struct AgentAccountMetaPointer {
    pub has_meta: bool,
}

impl Serializer for AgentAccountMetaPointer {
    fn read(reader: &mut Reader) -> Result<Self, ReaderError> {
        Ok(Self {
            has_meta: bool::read(reader)?,
        })
    }

    fn write(&self, writer: &mut Writer) {
        self.has_meta.write(writer);
    }

    fn size(&self) -> usize {
        self.has_meta.size()
    }
}
