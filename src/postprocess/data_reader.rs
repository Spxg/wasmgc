use super::Result;
use crate::schema::Layout;

pub struct DataReader<'a> {
    pub layout: Layout,
    pub segments: Vec<Option<(u64, &'a [u8])>>,
}

impl DataReader<'_> {
    pub fn address(&self, address: u64, offset: u64) -> Result<u64> {
        address
            .checked_add(offset)
            .ok_or_else(|| "wasmgc descriptor address overflow".into())
    }

    pub fn read<const N: usize>(&self, address: u64) -> Result<[u8; N]> {
        let mut result = [0; N];

        for (offset, byte) in result.iter_mut().enumerate() {
            let address = address
                .checked_add(offset as u64)
                .ok_or("wasmgc descriptor address overflow")?;

            // Later active segments overwrite earlier ones, including partial overlaps.
            // Resolve each byte independently rather than assuming one containing segment.
            *byte = self
                .segments
                .iter()
                .rev()
                .flatten()
                .find_map(|(start, bytes)| {
                    let offset = usize::try_from(address.checked_sub(*start)?).ok()?;

                    bytes.get(offset).copied()
                })
                .ok_or("wasmgc descriptor points outside initialized data")?;
        }

        Ok(result)
    }

    pub fn read_at<const N: usize>(&self, address: u64, offset: u32) -> Result<[u8; N]> {
        self.read(self.address(address, u64::from(offset))?)
    }

    pub fn read_u32(&self, address: u64, offset: u32) -> Result<u32> {
        Ok(u32::from_le_bytes(self.read_at(address, offset)?))
    }

    pub fn string(&self, address: u64, length: u32) -> Result<String> {
        let mut bytes = Vec::new();
        for offset in 0..length {
            bytes.push(self.read_at::<1>(address, offset)?[0]);
        }
        Ok(String::from_utf8(bytes)?)
    }

    pub fn pointer(&self, address: u64, offset: u32) -> Result<u64> {
        if self.layout.pointer_size == 8 {
            Ok(u64::from_le_bytes(self.read_at(address, offset)?))
        } else {
            Ok(u64::from(self.read_u32(address, offset)?))
        }
    }
}
