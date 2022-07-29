use std::{
    env,
    io::{stdin, stdout, Read, Write},
    path::PathBuf,
};

use multibufferedfile::BufferedFile;

pub fn main() {
    let mut args = env::args();
    assert_eq!(args.len(), 3);
    args.next();

    let verb = args
        .next()
        .expect("The first argument should be either read or write");
    let file = PathBuf::from(
        args.next()
            .expect("The second argument should be a file path"),
    );

    let buffered = BufferedFile::new(file).expect("Cloud not create file wrapper.");
    match verb.to_ascii_lowercase().as_str() {
        "read" => {
            let reader = buffered.read().expect("Could not create Reader");
            let stdout = stdout().lock();
            transfer(reader, stdout)
        }
        "write" => {
            let writer = buffered.write().expect("Could not create Reader");
            let stdin = stdin().lock();
            transfer(stdin, writer)
        }
        _ => panic!("The first argument should be either `read` or `write`"),
    }
}

fn transfer(mut rx: impl Read, mut tx: impl Write) {
    let mut buf = [0u8; 8192];
    loop {
        let count = rx.read(&mut buf).expect("Error reading from input");
        tx.write_all(&buf[0..count])
            .expect("Error writing to output");
        if count == 0 {
            break;
        }
    }
}
