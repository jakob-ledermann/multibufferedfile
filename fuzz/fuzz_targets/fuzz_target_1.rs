#![no_main]
use libfuzzer_sys::fuzz_target;
use multibufferedfile::BufferedFile;
use std::io::Write;

fuzz_target!(|data: &[u8]| {
    let temp_dir = utils::TempDir::new();

    if let Ok(file) = BufferedFile::new(temp_dir.path().join("fuzz_target_1.txt")) {
        file.write()
            .expect("should be writeable")
            .write_all(data)
            .expect("Error writing data");
    }
});

mod utils {
    use std::{
        env, fs,
        path::{Path, PathBuf},
    };

    #[derive(Debug)]
    pub struct TempDir(PathBuf);

    impl Drop for TempDir {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.0).unwrap();
        }
    }

    impl TempDir {
        /// Create a new empty temporary directory under the system's configured
        /// temporary directory.
        pub fn new() -> TempDir {
            use std::sync::atomic::{AtomicUsize, Ordering};

            static TRIES: usize = 100;
            #[allow(deprecated)]
            static COUNTER: AtomicUsize = AtomicUsize::new(0);

            let tmpdir = env::temp_dir();
            for _ in 0..TRIES {
                let count = COUNTER.fetch_add(1, Ordering::SeqCst);
                let path = tmpdir.join("rust-walkdir").join(count.to_string());
                if path.is_dir() {
                    continue;
                }
                fs::create_dir_all(&path)
                    .map_err(|e| panic!("failed to create {}: {}", path.display(), e))
                    .unwrap();
                return TempDir(path);
            }
            panic!("failed to create temp dir after {} tries", TRIES)
        }

        /// Return the underlying path to this temporary directory.
        pub fn path(&self) -> &Path {
            &self.0
        }
    }
}
