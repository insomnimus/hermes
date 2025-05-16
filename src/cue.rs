mod error;
mod parser;

use std::collections::BTreeMap;

use anyhow::{
	anyhow,
	Result,
};

#[derive(Debug, Clone, Default)]
pub struct Cue {
	pub rems: BTreeMap<String, String>,
	pub title: Option<String>,
	pub performer: Option<String>,
	pub songwriter: Option<String>,
	pub catalog: Option<String>,
	pub discs: Vec<Disc>,
}

#[derive(Debug, Clone, Default)]
pub struct Disc {
	pub rems: BTreeMap<String, String>,
	pub catalog: Option<String>,
	pub performer: Option<String>,
	pub songwriter: Option<String>,
	pub title: Option<String>,

	pub file: String,
	pub tracks: Vec<Track>,
}

#[derive(Debug, Clone, Default)]
pub struct Track {
	pub number: u32,
	pub title: Option<String>,
	pub performer: Option<String>,
	pub songwriter: Option<String>,
	pub isrc: Option<String>,
	pub index: u64,
	pub rems: BTreeMap<String, String>,
}

pub fn parse(cuesheet: &str) -> Result<Cue> {
	let lines = cuesheet.lines().collect::<Vec<_>>();
	parser::Parser::new(&lines)
		.parse()
		.map_err(|e| anyhow!("line {}: {}\n> {}", e.ln + 1, e.msg, lines[e.ln]))
}
