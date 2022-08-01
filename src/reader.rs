use std::io::{Read, Seek, SeekFrom};

///
/// Represents the read-only access to the file.
/// Validation has been performed on open. This provides an `impl std::io::Read` to the contents of the file.
/// 
#[derive(Debug)]
pub struct BufferedFileReader<T>
where
    T: Read,
{
    inner: T,
    useful_file_size: u64,
    pos: u64,
}

impl<T: Read + Seek> BufferedFileReader<T> {
    pub(crate) fn new(inner: T, len: u64) -> BufferedFileReader<T> {
        BufferedFileReader {
            inner,
            useful_file_size: len,
            pos: 0,
        }
    }
}

impl<T: Read> Read for BufferedFileReader<T> {
    fn read(&mut self, mut buf: &mut [u8]) -> std::io::Result<usize> {
        let limit = usize::try_from(self.useful_file_size - self.pos).unwrap_or(0);
        if buf.len() > limit {
            buf = &mut buf[..limit]
        }
        let read = self.inner.read(buf)?;
        self.pos = self.pos.saturating_add(
            u64::try_from(read)
                .expect("buffer len should fit into a u64. see calculation of limit above."),
        );
        Ok(read)
    }
}

impl<T: Seek + Read> Seek for BufferedFileReader<T> {
    fn seek(&mut self, pos: SeekFrom) -> std::io::Result<u64> {
        let inner_pos = match pos {
            SeekFrom::Start(start) => SeekFrom::Start(start.saturating_add(1)),
            SeekFrom::Current(delta) => SeekFrom::Current(delta),
            SeekFrom::End(distance) => SeekFrom::End(distance.saturating_add(4)),
        };

        let new_start = self.inner.seek(inner_pos)?.saturating_sub(1);
        self.pos = new_start;
        Ok(new_start)
    }
}

#[cfg(test)]
mod tests {
    use std::io::{Cursor, Read, Seek, SeekFrom};

    use crate::BufferedFileReader;

    #[test]
    fn simple() {
        let data = b"\0Hello world";
        let mut inner = Cursor::new(data);
        inner
            .seek(SeekFrom::Start(1))
            .expect("Cursor should be seekable");
        let mut reader = BufferedFileReader::new(inner, u64::try_from(data.len() - 1).unwrap());
        let mut content = Vec::new();
        reader
            .read_to_end(&mut content)
            .expect("Should be able to read");
        assert_eq!(&data[1..], content.as_slice())
    }

    #[test]
    fn partial_read() {
        let data = b"\0Hello world";
        let mut inner = Cursor::new(data);
        inner
            .seek(SeekFrom::Start(1))
            .expect("Cursor should be seekable");
        let mut reader = BufferedFileReader::new(inner, u64::try_from(data.len() - 1).unwrap());
        let mut content = [0u8; 10];
        reader
            .read_exact(&mut content)
            .expect("Should be able to read");

        assert_eq!(&data[1..11], content.as_slice());
        let count = reader.read(&mut content).expect("Should be able to read");

        assert_eq!(count, 1);
        assert_eq!(&data[11], &content[0])
    }
}
