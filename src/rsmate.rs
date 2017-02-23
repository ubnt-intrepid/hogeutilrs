extern crate rustc_serialize;
extern crate docopt;
extern crate memmap;
#[cfg(not(windows))]
extern crate nix;

use std::process::exit;
use docopt::Docopt;
#[cfg(not(windows))]
use nix::unistd::{fork, ForkResult};

const USAGE: &'static str = "
Rmate client written in Rust.

Usage:
  rmate [--host=<H> --port=<P> -w] <name>
  rmate -h | --help
  rmate -v | --version

Options:
  -h --help     Show this message.
  -v --version  Print version of information.
  --host=<H>    The hostname of Rmate server [default: localhost].
  --port=<P>    The port number of Rmate server [default: 52689].
  -w --wait     Wait for file to be closed by Textmate.
";

const HOST: &'static str = "localhost";
const PORT: u32 = 52689;

#[derive(Debug, RustcDecodable)]
struct Args {
  arg_name: Option<String>,
  arg_host: Option<String>,
  arg_port: Option<u32>,
  flag_wait: bool,
}

#[derive(Debug)]
pub struct Options {
  pub name: String,
  pub host: String,
  pub port: u32,
  pub wait: bool,
}

pub fn parse_options() -> Options {
  let args: Args = Docopt::new(USAGE)
    .and_then(|d| d.decode())
    .unwrap_or_else(|e| e.exit());

  if args.arg_name.is_none() {
    println!("filename is not given.");
    exit(1);
  }

  Options {
    name: args.arg_name.unwrap(),
    host: args.arg_host.unwrap_or(HOST.to_owned()),
    port: args.arg_port.unwrap_or(PORT),
    wait: args.flag_wait,
  }
}

#[cfg(windows)]
fn _fork() {}

#[cfg(not(windows))]
fn _fork() {
  match fork() {
    Ok(ForkResult::Parent { .. }) => exit(0),
    Ok(ForkResult::Child) => (),
    Err(_) => panic!("fork failed"),
  }
}

fn main() {
  let options = parse_options();
  println!("{:?}", options);

  if !options.wait {
    _fork()
  }

  // create a connection to Rmate server.
  let mut stream =
    std::net::TcpStream::connect(format!("{}:{}", options.host, options.port).as_str()).unwrap();

  // send all of the content to the server.
  rmate::send_open(&mut stream, options.name.as_str()).unwrap();

  // handle all commands
  let mut reader = std::io::BufReader::new(stream);

  let servername = {
    use std::io::BufRead;
    let mut reader = &mut reader;
    let mut line = String::new();
    reader.read_line(&mut line).unwrap();
    line.trim_right().to_owned()
  };
  println!("{:?}", servername);

  rmate::handle_commands(reader).unwrap();
}

mod rmate {
  use std::io::{self, BufRead, Write};
  use std::string::FromUtf8Error;
  use std::num::ParseIntError;
  use std::fs::canonicalize;
  use memmap::{Mmap, Protection};

  #[derive(Debug)]
  pub enum Error {
    Io(io::Error),
    FromUtf8(FromUtf8Error),
    ParseInt(ParseIntError),
    Parse(String),
  }

  impl From<io::Error> for Error {
    fn from(err: io::Error) -> Error {
      Error::Io(err)
    }
  }

  impl From<FromUtf8Error> for Error {
    fn from(err: FromUtf8Error) -> Error {
      Error::FromUtf8(err)
    }
  }

  impl From<ParseIntError> for Error {
    fn from(err: ParseIntError) -> Error {
      Error::ParseInt(err)
    }
  }

  pub type RMateResult<T> = Result<T, Error>;


  #[derive(Debug)]
  pub enum Cmd {
    Save,
    Close,
  }

  #[derive(Debug)]
  pub struct Command {
    pub cmd: Cmd,
    pub token: String,
    pub data: String,
  }

  pub fn send_open<W: Write>(stream: &mut W, name: &str) -> RMateResult<()> {
    let file_mmap = Mmap::open_path(name, Protection::Read)?;

    stream.write(b"open\n")?;
    stream.write(format!("display-name: {}\n", name).as_bytes())?;
    stream.write(format!("real-path: {:?}\n", canonicalize(name)?).as_bytes())?;
    stream.write(b"data-on-save: yes\n")?;
    stream.write(b"re-activate: yes\n")?;
    stream.write(format!("token: {}\n", name).as_bytes())?;
    stream.write(format!("data: {}\n", file_mmap.len()).as_bytes())?;
    stream.write(unsafe { file_mmap.as_slice() })?;
    stream.write(b"\n.\n")?;
    stream.flush()?;

    Ok(())
  }

  enum ReadCmd {
    Command(Command),
    Empty,
    Eof,
  }

  macro_rules! readline {
    ($reader:expr) => {
      {
        let mut line = String::new();
        let len = try!($reader.read_line(&mut line));
        if len == 0 { return Ok(ReadCmd::Eof); }
        line.trim_right().to_owned()
      }
    }
  }

  fn read_command<R: BufRead>(reader: &mut R) -> RMateResult<ReadCmd> {
    let cmd = readline!(reader);
    let cmd = match cmd.as_str() {
      "save" => Cmd::Save,
      "close" => Cmd::Close,
      _ => return Ok(ReadCmd::Empty),
    };

    let token = readline!(reader);
    let token = token.split(':')
      .nth(1)
      .map(|s| s.trim())
      .ok_or(Error::Parse("cannot parse token".to_owned()))?
      .to_owned();

    let len = readline!(reader);
    let len = len.split(':')
      .nth(1)
      .map(|s| s.trim())
      .ok_or(Error::Parse("cannot parse data length".to_owned()))?
      .to_owned();
    let len = len.parse::<usize>()?;

    let mut buf = Vec::with_capacity(len);
    buf.resize(len, 0u8);
    reader.read_exact(buf.as_mut_slice())?;
    let data = String::from_utf8(buf)?;

    Ok(ReadCmd::Command(Command {
      cmd: cmd,
      token: token,
      data: data,
    }))
  }

  pub fn handle_commands<R: BufRead>(mut reader: R) -> RMateResult<()> {
    loop {
      let command = match read_command(&mut reader)? {
        ReadCmd::Empty => continue,
        ReadCmd::Eof => break,
        ReadCmd::Command(command) => command,
      };
      println!("{:?}", command);

      match command.cmd {
        Cmd::Save => {
          use std::fs::OpenOptions;
          let mut file = OpenOptions::new().write(true).create(true).open(command.token)?;
          file.write_all(command.data.as_bytes())?;
        }
        Cmd::Close => {
          // do nothing
        }
      }
    }
    Ok(())
  }
}
