use std::{
    env::{self, temp_dir},
    eprintln, format,
    fs::{File, OpenOptions},
    io::{self, BufRead, BufReader, BufWriter, Read, Write},
    path::PathBuf,
    println, sync, thread,
    time::{Duration, Instant},
    writeln,
};

use base64::Engine;
use nix::{errno::Errno, sys::stat::Mode, unistd::mkfifo};

#[derive(Debug)]
struct Error {
    message: String,
}

impl Error {
    fn new(message: String) -> Error {
        Error { message }
    }
}

impl From<base64::DecodeError> for Error {
    fn from(value: base64::DecodeError) -> Self {
        let message = format!("Error on decode base64: {}", value.to_string());
        Self::new(message)
    }
}

impl From<io::Error> for Error {
    fn from(value: io::Error) -> Self {
        let message = format!("Error on input/output: {}", value.to_string());
        Self::new(message)
    }
}

impl From<&str> for Error {
    fn from(value: &str) -> Self {
        Self::new(value.to_string())
    }
}

type Result<T> = std::result::Result<T, Error>;

fn fifo_path() -> PathBuf {
    temp_dir().join("multi-i3status")
}

fn encode64(bytes: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

fn decode64(str: &str) -> Result<Vec<u8>> {
    let decoded = base64::engine::general_purpose::STANDARD.decode(str)?;
    Ok(decoded)
}

enum Config {
    Reader(i32),
    Reciever(f32),
    Both(i32, f32),
}

fn parse_config(args: &[String]) -> Option<Config> {
    if args.len() <= 1 {
        return None;
    }
    if args[1] == "reader" {
        let rank = args.get(2).map(|x| x.parse().unwrap()).unwrap_or(0);
        Some(Config::Reader(rank))
    } else if args[1] == "reciever" {
        let duration = args.get(2).map(|x| x.parse().unwrap()).unwrap_or(2.0);
        Some(Config::Reciever(duration))
    } else if args[1] == "both" {
        let rank = args.get(2).map(|x| x.parse().unwrap()).unwrap_or(0);
        let duration = args.get(3).map(|x| x.parse().unwrap()).unwrap_or(2.0);
        Some(Config::Both(rank, duration))
    } else {
        None
    }
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let Some(config) = parse_config(&args) else {
	panic!("Invalid arguments");
    };
    if let Err(err) = run(config) {
        eprintln!("{}", err.message);
    }
}

fn run(config: Config) -> Result<()> {
    match config {
        Config::Reader(priority) => reader(priority),
        Config::Reciever(duration) => reciever(duration),
        Config::Both(priority, duration) => {
            let (tx, rx) = sync::mpsc::channel();
            {
                let tx = tx.clone();
                thread::spawn(move || {
                    thread::sleep(Duration::from_secs(1));
                    if let Err(err) = reader(priority) {
                        eprintln!("{}", err.message);
                    }
                    tx.send(()).unwrap();
                })
            };
            {
                let tx = tx.clone();
                thread::spawn(move || {
                    let tx = tx.clone();
                    if let Err(err) = reciever(duration) {
                        eprintln!("{}", err.message);
                    }
                    tx.send(()).unwrap();
                });
            };
            rx.recv().unwrap();
            Ok(())
        }
    }
}

fn reciever(duration: f32) -> Result<()> {
    let path = fifo_path();
    match mkfifo(&path, Mode::S_IRWXU) {
        Ok(_) => (),
        Err(Errno::EEXIST) => (),
        Err(err) => {
            return Err(Error::new(format!(
                "Error on creating fifo file: {}",
                err.to_string()
            )));
        }
    };
    println!("{{\"version\":1}}");
    println!("[");
    let f = File::open(path)?;
    let mut reader = BufReader::new(f);
    let mut printed = false;
    let mut last_rank = -1;
    let mut last_update_time = Instant::now();
    loop {
        let mut buf = String::new();
        if reader.read_line(&mut buf)? == 0 {
            continue;
        }
        let (rank, body) = buf[0..buf.len() - 1]
            .split_once(":")
            .ok_or("There is no `:`")?;
        let body = decode64(body)?;
        let Ok(rank):std::result::Result<i32,_> = rank.parse() else{
	    eprintln!("Invalid rank: {}", rank);
	    continue;
	};
        let current_time = last_update_time.elapsed();
        if rank >= last_rank || current_time.as_secs_f32() > duration {
            if printed {
                print!(",");
            }
            printed = true;
            last_rank = rank;
            last_update_time = Instant::now();
            std::io::stdout().write_all(&body)?;
            std::io::stdout().flush()?;
        }
    }
}

fn reader(priority: i32) -> Result<()> {
    let path = fifo_path();
    let get_writer = || {
        let Ok(f) = OpenOptions::new().write(true).open(&path) else {
	    return None;
	};
        Some(BufWriter::new(f))
    };
    let mut writer = get_writer();
    let mut buffer = Vec::new();
    let mut nest = Vec::new();
    loop {
        if writer.is_none() {
            writer = get_writer();
        }
        let mut buf = [0; 1];
        loop {
            let n = io::stdin().read(&mut buf)?;
            if n == 1 {
                break;
            }
        }
        let c = buf[0];
        buffer.push(c);
        match c {
            _ if nest.last() == Some(&'\\') => (),
            b'"' => {
                if nest.last() == Some(&'"') {
                    nest.pop();
                } else {
                    nest.push('"');
                }
            }
            // remove separator of array
            b',' if nest == ['['] => buffer.clear(),
            b'{' => nest.push('{'),
            b'}' => {
                assert!(nest.last() == Some(&'{'));
                nest.pop();
                if nest.len() == 0 {
                    // omit {"version": 1}
                    buffer.clear();
                }
            }
            b'[' => {
                nest.push('[');
                if nest.len() == 1 {
                    // omit first letter of array
                    buffer.clear();
                }
            }
            b']' => {
                assert!(nest.last() == Some(&'['));
                nest.pop();
                if nest == ['['] {
                    buffer.push(b'\n');
                    if let Some(ref mut w) = writer {
                        let err = 'b: {
                            if let Err(err) = writeln!(w, "{}:{}", priority, encode64(&buffer)) {
                                eprintln!("Error on write to file: {}", err.to_string());
                                writer = None;
                                break 'b true;
                            }
                            if let Err(err) = w.flush() {
                                eprintln!("Error on write to file: {}", err.to_string());
                                writer = None;
                                break 'b true;
                            }
                            false
                        };
                        if err {
                            writer = None;
                        }
                    } else {
                        eprintln!("Output fails due to missing fifo file");
                    }
                    buffer.clear();
                }
            }
            _ => (),
        }
    }
}
