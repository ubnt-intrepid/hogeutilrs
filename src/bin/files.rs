extern crate clap;
extern crate mioco;
extern crate regex;
#[macro_use]
extern crate hogeutilrs;

use std::{env, fs, io, path};
use mioco::sync::mpsc;

use std::borrow::{Borrow, Cow};
use std::io::Write;
use std::ops::Deref;
use std::path::{Path, PathBuf};
use std::sync::Arc;


#[derive(Debug)]
enum FilesError {
  Regex(regex::Error),
  IO(io::Error),
  StripPrefix(path::StripPrefixError),
  Other(String),
}

def_from! { FilesError, regex::Error           => Regex }
def_from! { FilesError, io::Error              => IO }
def_from! { FilesError, path::StripPrefixError => StripPrefix }
def_from! { FilesError, String                 => Other }


#[derive(Debug)]
struct Cli {
  matchre: Option<regex::Regex>,
  ignore: Arc<Option<regex::Regex>>,
  is_async: bool,
}

impl Cli {
  fn build_cli() -> clap::App<'static, 'static> {
    let program = env::args()
      .nth(0)
      .and_then(|s| {
        PathBuf::from(s)
          .file_stem()
          .map(|s| s.to_string_lossy().into_owned())
      })
      .unwrap();

    use clap::{App, AppSettings, Arg};
    App::new(program)
      .about("find files")
      .version("0.1.0")
      .author("")
      .setting(AppSettings::VersionlessSubcommands)
      .arg(Arg::from_usage("-i --ignore=[IGNORE] 'Ignored pattern'"))
      .arg(Arg::from_usage("-m --matches=[IGNORE] 'pattern to match'"))
      .arg(Arg::from_usage("-a --async 'search asynchronously'"))
  }

  pub fn new() -> Result<Cli, FilesError> {
    let matches = Self::build_cli().get_matches();

    let ignore: Cow<str> = matches.value_of("ignore")
      .map(Into::into)
      .or(env::var("FILES_IGNORE_PATTERN").ok().map(Into::into))
      .unwrap_or(r#"^(\.git|\.hg|\.svn|_darcs|\.bzr)$"#.into());
    let ignore = if (ignore.borrow() as &str) != "" {
      Some(regex::Regex::new(ignore.borrow())?)
    } else {
      None
    };

    let matchre = match matches.value_of("matches") {
      Some(s) => Some(regex::Regex::new(s)?),
      None => None,
    };

    Ok(Cli {
      matchre: matchre,
      ignore: Arc::new(ignore),
      is_async: matches.is_present("async"),
    })
  }

  pub fn run(&mut self) -> Result<(), FilesError> {
    let root = env::current_dir()?;
    let rx = self.files(&root, self.is_async);

    loop {
      match rx.recv() {
        Ok(entry) => {
          if let Some(ref m) = self.matchre {
            if !m.is_match(entry.file_name().to_str().ok_or("".to_owned())?) {
              continue;
            }
          }
          println!("./{}",
                   entry.path()
                     .strip_prefix(&root)?
                     .display());
        }
        Err(_) => break,
      }
    }

    Ok(())
  }

  // Scan all files/directories under given directory synchronously
  fn files<P: Into<PathBuf>>(&self, root: P, is_async: bool) -> mpsc::Receiver<fs::DirEntry> {
    let root = root.into();
    let ignore = self.ignore.clone();

    let (tx, rx) = mpsc::sync_channel(40);
    let _ = mioco::spawn(move || files_inner(&root, tx, ignore, is_async));

    rx
  }
}

fn files_inner(entry: &Path,
               tx: mpsc::SyncSender<fs::DirEntry>,
               ignore: Arc<Option<regex::Regex>>,
               is_async: bool)
               -> Result<(), FilesError> {
  if is_match(&entry, ignore.deref()) {
    return Ok(());
  }

  for entry in std::fs::read_dir(entry)? {
    let entry = entry?;
    if !entry.path().is_dir() {
      if !is_match(&entry.path(), ignore.deref()) {
        let _ = tx.send(entry);
      }

    } else {
      let tx = tx.clone();
      let ignore = ignore.clone();

      if is_async {
        let _ = mioco::spawn(move || files_inner(&entry.path(), tx, ignore, is_async));
      } else {
        files_inner(&entry.path(), tx, ignore, is_async)?;
      }
    }
  }

  Ok(())
}

fn is_match(entry: &Path, pattern: &Option<regex::Regex>) -> bool {
  match *pattern {
    Some(ref pattern) => {
      if let Some(filename) = entry.file_name() {
        let filename = filename.to_string_lossy();
        pattern.is_match(filename.borrow())
      } else {
        false
      }
    }
    None => false,
  }
}

fn main() {
  mioco::start(|| -> Result<(), FilesError> {
      writeln!(&mut std::io::stderr(), "thread_num={}", mioco::thread_num())?;
      Ok(Cli::new()?.run()?)
    })
    .unwrap()
    .unwrap_or_else(|e| panic!("error: {:?}", e));
}
