// use core::fmt;

#[derive(Debug)]
pub struct Error {
	pub ln: usize,
	pub msg: anyhow::Error,
}

// impl std::error::Error for Error {}

// impl fmt::Display for Error {
// fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
// write!(f, "line {}: {:?}", self.ln + 1, self.msg)
// }
// }

pub trait ErrorCtx<T> {
	fn line(self, ln: usize) -> Result<T, Error>;
}

impl<T> ErrorCtx<T> for Result<T, anyhow::Error> {
	fn line(self, ln: usize) -> Result<T, Error> {
		self.map_err(|msg| Error { ln, msg })
	}
}
