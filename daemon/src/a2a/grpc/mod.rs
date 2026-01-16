pub mod proto {
    tonic::include_proto!("tos.a2a");
    pub const FILE_DESCRIPTOR_SET: &[u8] = tonic::include_file_descriptor_set!("a2a_descriptor");
}

pub mod service;
