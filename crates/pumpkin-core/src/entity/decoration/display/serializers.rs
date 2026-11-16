//! Tracked-data serializers for the display entity's vector and quaternion values.

use pumpkin_protocol::{
    java::client::play::MetadataSerializer,
    ser::{NetworkWriteExt, WritingError},
};

#[derive(Clone, Copy, Debug)]
pub struct Vector3fSerializer(pub f32, pub f32, pub f32);

impl MetadataSerializer for Vector3fSerializer {
    fn write_metadata(
        &self,
        writer: &mut impl std::io::Write,
        _version: &pumpkin_util::version::JavaMinecraftVersion,
    ) -> Result<(), WritingError> {
        writer.write_f32(self.0)?;
        writer.write_f32(self.1)?;
        writer.write_f32(self.2)
    }
}

#[derive(Clone, Copy, Debug)]
pub struct QuaternionfSerializer(pub f32, pub f32, pub f32, pub f32);

impl MetadataSerializer for QuaternionfSerializer {
    fn write_metadata(
        &self,
        writer: &mut impl std::io::Write,
        _version: &pumpkin_util::version::JavaMinecraftVersion,
    ) -> Result<(), WritingError> {
        writer.write_f32(self.0)?;
        writer.write_f32(self.1)?;
        writer.write_f32(self.2)?;
        writer.write_f32(self.3)
    }
}
