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
