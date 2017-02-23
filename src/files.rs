extern crate clap;
extern crate regex;

use std::{env, fs, io, thread};
use std::borrow::{Borrow, Cow};
use std::ops::Deref;
use std::path::{Path, PathBuf, StripPrefixError};
use std::sync::{Arc, mpsc};

#[macro_export]
macro_rules! def_from {
  ($t:ident, $src:ty => $dst:ident) => {
    impl From<$src> for $t {
      fn from(err: $src) -> $t {
        $t::$dst(err)
      }
    }
  }
}

#[derive(Debug)]
enum FilesError {
  Regex(regex::Error),
  IO(io::Error),
  StripPrefix(StripPrefixError),
  Other(String),
}
def_from! { FilesError, regex::Error     => Regex }
def_from! { FilesError, io::Error        => IO }
def_from! { FilesError, StripPrefixError => StripPrefix }
def_from! { FilesError, String           => Other }


#[derive(Debug)]
struct Cli {
  matchre: Option<regex::Regex>,
  ignore: Arc<Option<regex::Regex>>,
  is_async: bool,
  is_directory: bool,
  is_absolute: bool,
  max_items: usize,
}

impl Cli {
  fn build_app() -> clap::App<'static, 'static> {
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
      .version("0.0.1")
      .author("Yusuke Sasaki <yusuke.sasaki.nuem@gmail.com>")
      .setting(AppSettings::VersionlessSubcommands)
      .arg(Arg::from_usage("-i --ignore=[IGNORE]   'Ignored pattern'"))
      .arg(Arg::from_usage("-m --matches=[MATCHES] 'Pattern to match'"))
      .arg(Arg::from_usage("-a --absolute          'Show absolute path'"))
      .arg(Arg::from_usage("-d --directory         'Show only directories'"))
      .arg(Arg::from_usage("-A --async             'Search asynchronously'"))
      .arg(Arg::from_usage("-M --max-items=[N]     'Limit of displayed items'"))
  }

  pub fn new() -> Result<Cli, FilesError> {
    let matches = Self::build_app().get_matches();

    let matchre = match matches.value_of("matches") {
      Some(s) => Some(regex::Regex::new(s)?),
      None => None,
    };

    let ignore: Cow<str> = matches.value_of("ignore")
      .map(Into::into)
      .or(env::var("FILES_IGNORE_PATTERN").ok().map(Into::into))
      .unwrap_or(r#"^(\.git|\.hg|\.svn|_darcs|\.bzr)$"#.into());
    let ignore = if (ignore.borrow() as &str) != "" {
      Some(regex::Regex::new(ignore.borrow())?)
    } else {
      None
    };
    let ignore = Arc::new(ignore);

    let max_items =
      matches.value_of("max-items").and_then(|s| s.parse().ok()).unwrap_or(usize::max_value());

    Ok(Cli {
      matchre: matchre,
      ignore: ignore,
      is_directory: matches.is_present("directory"),
      is_absolute: matches.is_present("absolute"),
      is_async: matches.is_present("async"),
      max_items: max_items,
    })
  }

  pub fn run(&mut self) -> Result<(), FilesError> {
    let root = env::current_dir()?;

    for entry in self.files(&root)
      .into_iter()
      .filter(|entry| !self.matchre.is_some() || is_match(&entry.path(), &self.matchre))
      .take(self.max_items) {

      if self.is_absolute {
        println!("{}", entry.path().display());
      } else {
        println!("./{}",
                 entry.path()
                   .strip_prefix(&root)?
                   .display());
      }
    }

    Ok(())
  }

  // Scan all files/directories under given directory synchronously
  fn files<P: Into<PathBuf>>(&self, root: P) -> mpsc::Receiver<fs::DirEntry> {
    let root = root.into();
    let ignore = self.ignore.clone();
    let is_dir = self.is_directory;
    let is_async = self.is_async;

    let (tx, rx) = mpsc::sync_channel(20);
    thread::spawn(move || Self::files_inner(&root, tx, ignore, is_dir, is_async));

    rx
  }

  fn files_inner(entry: &Path,
                 tx: mpsc::SyncSender<fs::DirEntry>,
                 ignore: Arc<Option<regex::Regex>>,
                 is_dir: bool,
                 is_async: bool)
                 -> Result<(), FilesError> {
    if is_match(&entry, ignore.deref()) {
      return Ok(());
    }

    for entry in std::fs::read_dir(entry)? {
      let entry = entry?;
      if !entry.path().is_dir() {
        if !is_dir && !is_match(&entry.path(), ignore.deref()) {
          tx.send(entry).unwrap();
        }

      } else {
        let path = entry.path().to_owned();
        let tx = tx.clone();
        let ignore = ignore.clone();

        if is_dir {
          tx.send(entry).unwrap();
        }

        if is_async {
          thread::spawn(move || Self::files_inner(&path, tx, ignore, is_dir, is_async).unwrap());
        } else {
          Self::files_inner(&path, tx, ignore, is_dir, is_async)?;
        }
      }
    }

    Ok(())
  }
}

fn is_match(entry: &Path, pattern: &Option<regex::Regex>) -> bool {
  match *pattern {
    Some(ref pattern) => {
      let filename = entry.file_name()
        .unwrap()
        .to_string_lossy();
      pattern.is_match(filename.borrow())
    }
    None => false,
  }
}

fn _main() -> Result<(), FilesError> {
  Ok(Cli::new()?.run()?)
}

fn main() {
  _main().unwrap_or_else(|e| panic!("error: {:?}", e));
}
