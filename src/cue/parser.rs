use anyhow::{
	anyhow,
	bail,
	Result,
};

use super::{
	error::{
		Error,
		ErrorCtx,
	},
	Cue,
	Disc,
	Track,
};

macro_rules! err {
	[$ln:expr, $($args:tt)+] => {
		Err($crate::cue::error::Error {
			ln: $ln,
			msg: anyhow::anyhow!($($args)+),
		})
	};
}

pub struct Parser<'a> {
	lines: &'a [&'a str],
	ln: usize,
}

fn consume_space1(input: &str) -> Option<&str> {
	let i = input
		.bytes()
		.position(|c| c != b' ' && c != b'\t')
		.filter(|&i| i > 0)?;

	Some(&input[i..])
}

fn next_word(s: &str) -> Option<(&str, &str)> {
	let start = s.find(|c: char| !c.is_ascii_whitespace())?;
	let s = &s[start..];
	let end = s.find(|c: char| c.is_ascii_whitespace()).unwrap_or(s.len());

	Some((&s[end..], &s[..end]))
}

fn parse_index(input: &str) -> Result<u64> {
	let (input, _number) = next_word(input).ok_or_else(|| anyhow!("missing index number"))?;
	let input = consume_space1(input)
		.ok_or_else(|| anyhow!("missing time specifier after index number"))?;
	let word = parse_val(input)?;

	let nums = word.rsplit(':');

	let mut n = 0;
	for (field, multiplier) in nums.zip([1, 1000, 60000, 60 * 60000]) {
		n += multiplier
			* field
				.parse::<u64>()
				.map_err(|_| anyhow!("invalid index time: {word}"))?;
	}

	Ok(n)
}

fn escaped(c: char) -> char {
	match c {
		'n' => '\n',
		't' => '\t',
		'r' => '\r',
		_ => c,
	}
}

fn parse_str(input: &str) -> Result<(&str, String)> {
	let input = input.trim_start();
	let mut buf = String::new();
	let mut chars = input.char_indices();

	if !input.starts_with('"') {
		let mut end = 0;
		while let Some((i, c)) = chars.next() {
			match c {
				' ' | '\t' | '\r' | '\n' => break,
				'\\' => match chars.next() {
					Some((i, esc)) => {
						buf.push(escaped(esc));
						end = i;
						continue;
					}
					None => {
						buf.push('\\');
						return Ok(("", buf));
					}
				},
				_ => buf.push(c),
			}

			end = i;
		}

		debug_assert!(!buf.is_empty());
		return Ok((&input[end + 1..], buf));
	}

	let _ = chars.next().unwrap();
	while let Some((i, c)) = chars.next() {
		match c {
			'"' => return Ok((&input[i + 1..], buf)),
			'\\' => {
				let Some((_, esc)) = chars.next() else {
					break;
				};
				buf.push(escaped(esc));
			}
			_ => buf.push(c),
		}
	}

	Err(anyhow!("unterminated double-quoted string"))
}

fn parse_rem(input: &str) -> Result<(String, String)> {
	let input = input.trim_start();
	if input.is_empty() {
		bail!("expected 2 values, have none");
	}

	let (rest, key) = parse_str(input)?;

	let Some(i) = rest.find(|c: char| c != ' ' && c != '\t') else {
		return Ok((key, String::new()));
	};
	// .filter(|&i| i > 0);

	let val = parse_val(&rest[i..])?.trim().to_string();
	Ok((key, val))
}

fn parse_val(input: &str) -> Result<String> {
	let input = input.trim();
	if input.is_empty() {
		bail!("missing value");
	} else if input.len() > 1 && input.starts_with('"') && input.ends_with('"') {
		let (rest, val) = parse_str(input).map_err(|e| anyhow!("{e}"))?;
		if !rest.trim().is_empty() {
			bail!("too many values in line");
		}

		Ok(val)
	} else {
		Ok(input.to_string())
	}
}

impl<'a> Iterator for Parser<'a> {
	// (line_no, field_name, field_value)
	type Item = (usize, &'a str, &'a str);

	fn next(&mut self) -> Option<Self::Item> {
		while self.ln < self.lines.len() {
			let i = self.ln;
			let s = self.lines[i];
			self.ln += 1;

			let Some((rest, field)) = next_word(s) else {
				continue;
			};
			// println!("{i}:{s}");
			return Some((i, field, rest));
		}

		None
	}
}

impl<'a> Parser<'a> {
	pub fn new(lines: &'a [&'a str]) -> Self {
		Self { lines, ln: 0 }
	}

	fn is_exhausted(&self) -> bool {
		self.ln >= self.lines.len()
	}

	pub fn parse(mut self) -> Result<Cue, Error> {
		let mut cue = Cue::default();

		// Parse global declarations
		for (ln, field, val) in &mut self {
			match field.to_lowercase().as_str() {
				"rem" => {
					let (k, v) = parse_rem(val).line(ln)?;
					cue.rems.insert(k, v);
				}
				"title" => cue.title = Some(parse_val(val).line(ln)?),
				"performer" => cue.performer = Some(parse_val(val).line(ln)?),
				"catalog" => cue.catalog = Some(parse_val(val).line(ln)?),
				"songwriter" => cue.songwriter = Some(parse_val(val).line(ln)?),

				"file" => {
					self.ln = ln;
					break;
				}
				"track" => return err!(ln, "`TRACK` declared before any `FILE`"),
				_ => return err!(ln, "unknown field for a disc: {field}"),
			}
		}

		// Parse discs
		if self.is_exhausted() {
			return err!(0, "cue sheet is missing a `FILE` declaration");
		}

		while !self.is_exhausted() {
			cue.discs.push(self.parse_disc()?);
		}

		Ok(cue)
	}

	fn parse_disc(&mut self) -> Result<Disc, Error> {
		let (_, field, rest) = self.next().unwrap();
		debug_assert_eq!("file", &field.to_lowercase());

		let (_kind, file) = parse_str(rest).line(self.ln)?;

		let mut disc = Disc {
			file,
			..Disc::default()
		};

		// Any declaration before the first `TRACK` applies to the disc
		while let Some((ln, field, val)) = self.next() {
			match field.to_lowercase().as_str() {
				"track" => {
					self.ln = ln;
					break;
				}
				"rem" => {
					let (k, v) = parse_rem(val).line(ln)?;
					disc.rems.insert(k, v);
				}
				"title" => disc.title = Some(parse_val(val).line(ln)?),
				"performer" => disc.performer = Some(parse_val(val).line(ln)?),
				"songwriter" => disc.songwriter = Some(parse_val(val).line(ln)?),
				"catalog" => disc.catalog = Some(parse_val(val).line(ln)?),
				"file" => {
					self.ln = ln;
					return Ok(disc);
				}
				_ => return err!(ln, "unknown field for disc: {field}"),
			}
		}

		// Parse tracks
		if self.is_exhausted() {
			return Ok(disc);
		}

		while let Some((ln, field, val)) = self.next() {
			debug_assert_eq!("track", &field.to_lowercase());

			let (_kind, no) = parse_str(val).line(ln)?;
			let no = no
				.parse::<u32>()
				.map_err(|_| anyhow!("invalid track number"))
				.line(ln)?;

			let mut track = Track {
				number: no,
				..Track::default()
			};

			let mut have_index = false;
			let track_ln = ln;

			while let Some((ln, field, val)) = self.next() {
				match field.to_lowercase().as_str() {
					"track" => {
						self.ln = ln;
						break;
					}
					"file" => {
						if !have_index {
							return err!(track_ln, "track is missing a `INDEX` declaration");
						}
						disc.tracks.push(track);
						self.ln = ln;
						return Ok(disc);
					}
					"index" => {
						let idx = parse_index(val).line(ln)?;
						track.index = u64::max(track.index, idx);
						have_index = true;
					}
					"title" => track.title = Some(parse_val(val).line(ln)?),
					"performer" => track.performer = Some(parse_val(val).line(ln)?),
					"songwriter" => track.songwriter = Some(parse_val(val).line(ln)?),
					"isrc" => track.isrc = Some(parse_val(val).line(ln)?),
					"flags" => (),
					"rem" => {
						let (k, v) = parse_rem(val).line(ln)?;
						track.rems.insert(k, v);
					}
					_ => return err!(ln, "unknown field for a track: {field}"),
				}
			}

			if !have_index {
				return err!(track_ln, "track is missing a `INDEX` declaration");
			}

			disc.tracks.push(track);
		}

		Ok(disc)
	}
}
