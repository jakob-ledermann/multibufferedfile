use std::{io::Write, mem::ManuallyDrop};

use crc::Digest;

pub struct BufferedFileWriter<T: Write> {
    inner: T,
    digest: ManuallyDrop<Digest<'static, u32>>,
}

impl<T: Write> std::io::Write for BufferedFileWriter<T> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let count = self.inner.write(buf)?;
        self.digest.update(&buf[..count]);
        Ok(count)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.inner.flush()
    }
}

impl<T: Write> BufferedFileWriter<T> {
    pub(crate) fn new(target: T) -> Self {
        let digest = crate::CRC.digest();
        BufferedFileWriter {
            inner: target,
            digest: ManuallyDrop::new(digest),
        }
    }
}

impl<T: Write> Drop for BufferedFileWriter<T> {
    fn drop(&mut self) {
        // SAFETY: this is the only instance where the digest is removed so it is still valid.
        // this is drop so it can't be called more than once.
        let digest = unsafe { ManuallyDrop::take(&mut self.digest) };
        let checksum = digest.finalize();
        let _ = self.inner.write_all(&checksum.to_le_bytes());
    }
}

#[cfg(test)]
mod tests {
    use std::io::{Cursor, Write};

    use crate::BufferedFileWriter;

    #[test]
    fn simple() {
        const DATA: &[u8] = b"hello world";
        let mut buffer: Vec<u8> = Vec::new();
        let target = Cursor::new(&mut buffer);
        let checksum = crate::CRC.checksum(DATA);
        let mut writer = BufferedFileWriter::new(target);
        writer.write_all(DATA).expect("Should be writeable");
        drop(writer);

        let mut expected = Vec::new();
        expected.extend_from_slice(DATA);
        expected.extend_from_slice(&checksum.to_le_bytes());
        assert_eq!(buffer, expected);
    }
}
