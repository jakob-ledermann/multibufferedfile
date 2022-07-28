use std::{
    cmp::Ordering,
    fs::OpenOptions,
    io::{Read, Seek, SeekFrom, Write},
    mem::ManuallyDrop,
    path::{Path, PathBuf},
};

use crc::{Crc, Digest, CRC_32_ISCSI};
use thiserror::Error;

const BUFFER_COUNT: u8 = 2;
const BUFFER_SIZE: usize = 8192;

pub struct BufferedFile {
    file: std::path::PathBuf,
    current_generation: u8,
}

#[derive(Error, Debug)]
pub enum BufferedFileErrors {
    #[error("Error interacting with filesystem: '{0}")]
    IoError(#[from] std::io::Error),
    #[error("No valid file available")]
    AllFilesInvalidError,
}

enum FileCheckResult {
    Good { generation: u8 },
    ChecksumFailure,
}
const CRC: crc::Crc<u32> = Crc::<u32>::new(&CRC_32_ISCSI);

fn check_file(file: &Path) -> std::io::Result<FileCheckResult> {
    let mut file = std::fs::File::open(file)?;
    let mut digest = CRC.digest();
    let mut buf = [0u8; BUFFER_SIZE];
    let mut valid = file.read(&mut buf)?;
    let read = &buf[..valid];
    let generation = read[0];
    digest.update(&read[1..read.len() - 4]);
    let mut potential_expected_crc32: u32 = u32::from_le_bytes(
        read[read.len() - 4..]
            .try_into()
            .expect("I should have valid bytes available"),
    );
    loop {
        valid = file.read(&mut buf)?;
        match valid {
            0 => {
                // File is finished
                return Ok(if digest.finalize() == potential_expected_crc32 {
                    FileCheckResult::Good { generation }
                } else {
                    FileCheckResult::ChecksumFailure
                });
            }
            x if x < 4 => {
                todo!("not enough data available for a potential crc32 checksum")
            }
            _ => {
                let read = &buf[..valid];
                let (data, pot_checksum) = read.split_at(read.len() - 4);
                potential_expected_crc32 = u32::from_le_bytes(
                    pot_checksum
                        .try_into()
                        .expect("there should be 4 u8 available"),
                );
                digest.update(&potential_expected_crc32.to_le_bytes());
                digest.update(data);
            }
        }
    }
}

pub struct BufferedFileReader {
    inner: std::fs::File,
    useful_file_size: u64,
}

impl std::io::Read for BufferedFileReader {
    fn read(&mut self, mut buf: &mut [u8]) -> std::io::Result<usize> {
        let limit =
            usize::try_from(self.useful_file_size - self.inner.stream_position()?).unwrap_or(0);
        if buf.len() > limit {
            buf = &mut buf[..limit]
        }
        self.inner.read(buf)
    }
}

impl std::io::Seek for BufferedFileReader {
    fn seek(&mut self, pos: SeekFrom) -> std::io::Result<u64> {
        let inner_pos = match pos {
            SeekFrom::Start(start) => SeekFrom::Start(start.saturating_add(1)),
            SeekFrom::Current(delta) => SeekFrom::Current(delta),
            SeekFrom::End(distance) => SeekFrom::End(distance.saturating_add(4)),
        };

        let new_start = self.inner.seek(inner_pos)?.saturating_sub(1);
        Ok(new_start)
    }
}

pub use writer::*;

mod writer;

impl BufferedFile {
    pub fn new(path: std::path::PathBuf) -> Result<Self, BufferedFileErrors> {
        let files = Self::find_files(&path)?;
        Self::select_file(files.into_iter())
    }

    pub fn read(self) -> Result<BufferedFileReader, BufferedFileErrors> {
        let mut file = OpenOptions::new().read(true).open(self.file)?;
        file.seek(SeekFrom::Start(1))?;
        let usable_file_size = file.metadata()?.len().saturating_sub(4);
        Ok(BufferedFileReader {
            inner: file,
            useful_file_size: usable_file_size,
        })
    }

    pub fn write(self) -> Result<BufferedFileWriter<std::fs::File>, BufferedFileErrors> {
        let files = Self::find_files(&self.file)?;

        let mut files_with_generation: Vec<_> = files
            .iter()
            .flat_map(|path| {
                let file = OpenOptions::new().read(true).open(path);
                match file {
                    Ok(file) => Ok((path, file.bytes().next())),
                    Err(x) => Err(BufferedFileErrors::from(x)),
                }
            })
            .collect();

        files_with_generation.sort_unstable_by(|a, b| {
            let a = &a.1;
            let b = &b.1;

            match (a, b) {
                (Some(Ok(a)), Some(Ok(b))) => wrapping_cmp(*a, *b),
                (None, None) => Ordering::Equal,
                (_, Some(Ok(_))) => Ordering::Less,
                (Some(Ok(_)), _) => Ordering::Greater,
                (None, Some(Err(_))) => Ordering::Equal,
                (Some(Err(_)), Some(Err(_))) => Ordering::Equal,
                (Some(Err(_)), None) => Ordering::Equal,
            }
        });

        let target_file = match files_with_generation.first() {
            Some(target_file) => {
                let mut target_file = OpenOptions::new()
                    .write(true)
                    .create(true)
                    .truncate(true)
                    .open(target_file.0)?;
                target_file.write_all(&[self.current_generation.wrapping_add(1)])?;
                target_file
            }
            None => {
                let mut default_file_path = self.file;
                default_file_path.push(".1");
                let mut target_file = OpenOptions::new()
                    .write(true)
                    .create(true)
                    .truncate(true)
                    .open(default_file_path)?;
                target_file.write_all(&[1])?;
                target_file
            }
        };
        Ok(BufferedFileWriter::new(target_file))
    }

    fn select_file(
        files: impl Iterator<Item = PathBuf>,
    ) -> std::result::Result<BufferedFile, BufferedFileErrors> {
        let mut valid_files = Vec::with_capacity(BUFFER_COUNT.into());
        for file in files {
            match check_file(file.as_path())? {
                FileCheckResult::ChecksumFailure => {
                    continue;
                }
                FileCheckResult::Good { generation } => {
                    valid_files.push((file, generation));
                }
            }
        }

        valid_files.sort_by_cached_key(|(_file, generation)| *generation);

        match valid_files.into_iter().next() {
            Some((file, current_generation)) => Ok(Self {
                file,
                current_generation,
            }),
            None => Err(BufferedFileErrors::AllFilesInvalidError),
        }
    }

    fn find_files(path: &std::path::Path) -> std::io::Result<Vec<PathBuf>> {
        let stem = path
            .file_name()
            .expect("provided path should be a valid file path");
        let ancestor = path
            .ancestors()
            .next()
            .expect("provided path should be a valid file path");

        let mut result = Vec::with_capacity(BUFFER_COUNT.into());
        for i in 1..BUFFER_COUNT {
            let mut file = ancestor.to_path_buf();
            let mut file_name = stem.to_os_string();
            file_name.push(format!(".{i}"));
            file.push(file_name);

            if file.exists() {
                result.push(file);
            }
        }
        Ok(result)
    }
}

fn wrapping_cmp(a: u8, b: u8) -> Ordering {
    match a.wrapping_sub(b) {
        0 => Ordering::Equal,
        x if x < 128 => Ordering::Less,
        _ => Ordering::Greater,
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn it_works() {
        let result = 2 + 2;
        assert_eq!(result, 4);
    }
}
