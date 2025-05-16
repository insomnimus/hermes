mod cue;
mod preset;
mod template;

use std::{
	collections::{
		btree_map::Entry,
		BTreeMap,
	},
	fs,
	path::{
		Path,
		PathBuf,
	},
	process::Command,
	rc::Rc,
	sync::Arc,
};

use anyhow::{
	anyhow,
	bail,
	ensure,
	Result,
};
use clap::Parser;
use jwalk::WalkDir;
use rayon::prelude::*;

use crate::{
	cue::{
		Cue,
		Disc,
		Track,
	},
	preset::Preset,
	template::Template,
};

const TEMPLATE_VARS: &[&str] = &["title", "album", "artist", "no", "year", "ext", "dir-name"];

#[derive(Parser)]
/// Hermes splits cuesheet + image files into separate tracks.
///
/// Requires an ffmpeg executable.
#[command(version)]
struct Args {
	/// Path to a cuesheet file or a directory
	#[arg(group = "action")]
	path: Option<PathBuf>,
	/// Maximum number of parallel ffmpeg invocations; defaults to about half the available logical CPU cores
	#[arg(short, long)]
	jobs: Option<usize>,

	/// Do not actually split files; useful for checking if there will be errors
	#[arg(long)]
	dry: bool,
	/// Overwrite existing output files without asking
	#[arg(short, long)]
	force: bool,
	/// Ignore tracks that already exist in the filesystem without prompting
	#[arg(short, long, conflicts_with = "force")]
	no_overwrite: bool,

	/// Template string to determine file names
	#[arg(short, long, value_parser = parse_template, default_value = "<year> - <album>/<no>. <title>.<ext>")]
	template: Template,

	/// Output directory path; defaults to <cue_dir>/split
	#[arg(short, long)]
	out_dir: Option<PathBuf>,

	/// High-level preset for encoding (defaults to flac)
	#[arg(short, long)]
	preset: Option<Preset>,
	/// Do not attempt to avoid re-encoding
	#[arg(long)]
	no_copy: bool,

	/// Encoding options to pass to ffmpeg
	#[arg(
		short_alias = 'a',
		alias = "encode-arg",
		conflicts_with = "preset",
		requires = "ext",
		last = true
	)]
	encode_arg: Vec<String>,
	/// The extension without the leading dot, substituted in the template string
	#[arg(short, long, value_parser = validate_ext, conflicts_with = "preset", requires = "encode_arg")]
	ext: Option<String>,

	/// Path to the ffmpeg executable
	#[arg(long, default_value = "ffmpeg")]
	ffmpeg: PathBuf,

	/// Print help for the template syntax
	#[arg(long, group = "action")]
	template_help: bool,
	/// Show available presets
	#[arg(long, group = "action")]
	list_presets: bool,
}

struct Context<'a> {
	args: &'a Args,

	force_opt: Option<&'static str>,
	year: String,

	cue: Cue,
	dir: Arc<Path>,
}

struct Job {
	new_files: Vec<PathBuf>,
	cmd: Command,
}

fn parse_template(s: &str) -> Result<Template> {
	let template = Template::new(s, "<", ">");
	for s in template.vars() {
		if !TEMPLATE_VARS.contains(&s) {
			bail!("unrecognized template variable: <{s}>\nrun with --template-help for usage");
		}
	}

	Ok(template)
}

fn validate_ext(s: &str) -> Result<String, &'static str> {
	if s.is_empty() {
		Err("extension can't be empty")
	} else if s.contains(|c: char| !c.is_alphanumeric()) {
		Err("extensions must consist of alphanumeric characters only")
	} else {
		Ok(s.to_string())
	}
}

#[cold]
fn show_template_help() {
	println!(
		"\
You can use string templates to control generated file names.
Variables inside angle brackets <> will be replaced with values.
Allowed variables:
  - <artist>: Artist name
  - <album>: Album name
  - <title>: Song title
  - <no>: The song number, padded with zeroes to the left if necessary
  - <year>: The release year of the album
  - <dir-name>: Name of the directory containing the .cue file
  - <ext>: File extension without any leading dot

Any other variable is an error\
"
	);
}

fn normalize(s: &str) -> String {
	s.chars()
		.fold(String::with_capacity(s.len()), |mut buf, c| {
			match c {
				'"' => buf.push('\''),
				'<' => buf.push('〈'),
				'>' => buf.push('﹥'),
				':' => buf.push(' '),
				'/' | '\\' => buf.push('-'),
				'?' => (),
				'*' => buf.push('﹡'),
				_ => buf.push(c),
			}

			buf
		})
}

fn try_copy_codec(p: &Path) -> Option<&'static str> {
	const KNOWN_EXTS: &[&str] = &["wav", "flac", "mp3", "aac", "m4a", "opus", "ogg"];
	let ext = p.extension()?.to_str()?;
	KNOWN_EXTS
		.iter()
		.copied()
		.find(|s| s.eq_ignore_ascii_case(ext))
}

fn ms_to_ffmpeg(ms: u64) -> String {
	let sec = ms / 1000;
	let rem = ms - sec * 1000;

	if rem == 0 {
		sec.to_string()
	} else {
		format!("{sec}.{rem:03}")
	}
}

fn cue_md(c: &Cue) -> Vec<String> {
	let mut md = c
		.rems
		.iter()
		.map(|(k, v)| format!("{k}={v}"))
		.collect::<Vec<_>>();

	if let Some(artist) = &c.performer {
		md.push(format!("ARTIST={artist}"));
		md.push(format!("PERFORMER={artist}"));
	}

	if let Some(album) = &c.title {
		md.push(format!("ALBUM={album}"));
	}

	if let Some(sw) = &c.songwriter {
		md.push(format!("SONGWRITER={sw}"));
	}

	md
}

fn push_disc_md(d: &Disc, md: &mut Vec<String>) {
	md.extend(d.rems.iter().map(|(k, v)| format!("{k}={v}")));

	if let Some(artist) = &d.performer {
		md.push(format!("ARTIST={artist}"));
		md.push(format!("PERFORMER={artist}"));
	}

	if let Some(album) = &d.title {
		md.push(format!("ALBUM={album}"));
	}

	if let Some(sw) = &d.songwriter {
		md.push(format!("SONGWRITER={sw}"));
	}
}

fn push_track_md(t: &Track, md: &mut Vec<String>) {
	md.extend(t.rems.iter().filter_map(|(k, v)| {
		let v = v.trim();
		if !k.is_empty() && !v.is_empty() {
			Some(format!("{k}={v}"))
		} else {
			None
		}
	}));

	if let Some(title) = &t.title {
		md.push(format!("TITLE={title}"));
	}

	if let Some(artist) = &t.performer {
		md.push(format!("ARTIST={artist}"));
		md.push(format!("PERFORMER={artist}"));
	}

	if let Some(sw) = &t.songwriter {
		md.push(format!("SONGWRITER={sw}"));
	}

	if let Some(isrc) = &t.isrc {
		md.push(format!("ISRC={isrc}"));
	}

	md.push(format!("TRACKNUMBER={}", t.number));
}

fn parse_cue(p: &Path) -> Result<Cue> {
	// const BOM: char = '\u{FEFF}';
	let data = fs::read(p).map_err(|e| anyhow!("error reading {}: {}", p.display(), e))?;

	let mut detect = chardetng::EncodingDetector::new();
	detect.feed(&data, true);
	let mut dec = detect.guess(None, true).new_decoder();
	let mut buf = String::with_capacity(dec.max_utf8_buffer_length(data.len()).unwrap());
	buf.extend((0..buf.capacity()).map(|_| '\0'));
	let (res, _read, len, _has_replacement) = dec.decode_to_str(&data, &mut buf, true);
	debug_assert_eq!(res, encoding_rs::CoderResult::InputEmpty);
	cue::parse(&buf[..len]).map_err(|e| anyhow!("error parsing {}: {}", p.display(), e))
}

fn list_presets() {
	use clap::ValueEnum;
	for p in Preset::value_variants() {
		println!(
			"{}: {}",
			p.to_possible_value().unwrap().get_name(),
			p.ffmpeg_args().join(" "),
		);
	}
}

fn run() -> Result<()> {
	let mut args = Args::parse();
	if args.template_help {
		show_template_help();
		return Ok(());
	} else if args.list_presets {
		list_presets();
		return Ok(());
	}

	let path = args.path.take().unwrap();
	if !path.exists() {
		bail!("file or directory does not exist: {}", path.display());
	}

	if let Some(n) = args.jobs.or_else(|| {
		std::thread::available_parallelism()
			.ok()
			.map(|n| (n.get() / 2 + 1))
	}) {
		let _ = rayon::ThreadPoolBuilder::new()
			.num_threads(n)
			.build_global();
	}

	let cues = WalkDir::new(&path)
		.skip_hidden(false)
		.follow_links(true)
		.into_iter()
		.filter_map(|res| match res {
			Ok(entry)
				if entry.file_type.is_file()
					&& Path::new(&entry.file_name)
						.extension()
						.is_some_and(|s| s.eq_ignore_ascii_case("cue")) =>
			{
				let p = entry.parent_path.join(entry.file_name);
				let res = parse_cue(&p).map(move |cue| (cue, entry.parent_path, p));

				Some(res)
			}
			_ => None,
		})
		.collect::<Result<Vec<_>, _>>()?;

	ensure!(!cues.is_empty(), "no .cue files found");

	let force_opt = if args.force {
		Some("-y")
	} else if args.no_overwrite {
		Some("-n")
	} else {
		None
	};

	let need_album = args.template.contains_var("album");
	let need_year = args.template.contains_var("year");

	let mut jobs = Vec::with_capacity(cues.len());
	let mut new_files = BTreeMap::new();

	for (cue, dir, cue_path) in cues {
		for disc in &cue.discs {
			let to_split = dir.join(&disc.file);
			ensure!(
				to_split.exists(),
				"file specified in {} does not exist: {}",
				cue_path.display(),
				to_split.display()
			);
		}
		let mut year = String::new();
		if need_year {
			year = cue.rems.iter().find_map(|(k, v)| if !v.is_empty() && k.eq_ignore_ascii_case("DATE") {
				let year = v.split(['-', '.', '/', '\\']).max_by_key(|s| s.len()).filter(|s| !s.is_empty())?;
				// For validation
				let _ = year.parse::<u16>().ok()?;
				Some(normalize(year))
			} else {
				None
			})
			.ok_or_else(|| anyhow!("<year> template variable is used but the file {} does not contain date information", cue_path.display()))?;
		}

		if need_album && cue.title.is_none() {
			bail!("the <album> template variable is used but the cuesheet at {} does not contain a disc title", cue_path.display());
		}

		let ctx = Context {
			args: &args,
			force_opt,
			year,
			cue,
			dir,
		};

		let mut js = Job::new_jobs(ctx)
			.map_err(|e| anyhow!("error processing {}: {}", cue_path.display(), e))?;

		let cue_path = Rc::<Path>::from(cue_path);

		for f in js.iter().flat_map(|j| &j.new_files) {
			match new_files.entry(f.clone()) {
				Entry::Vacant(x) => _ = x.insert(Rc::clone(&cue_path)),
				Entry::Occupied(x) => {
					if *x.get() == cue_path {
						bail!("multiple tracks in the file {} will have the same file name: {}\nhelp: specify a different file naming scheme with the --template option",
						cue_path.display(),
						f.display(),
						);
					} else {
						bail!(
							"tracks from {} and {} have the same file name\nhelp: specify a different file naming scheme with the --template option",
							x.get().display(),
							cue_path.display(),
						);
					}
				}
			}
		}

		jobs.append(&mut js);
	}

	if args.dry {
		return Ok(());
	}

	jobs.into_par_iter().try_for_each(Job::run)?;
	Ok(())
}

impl Job {
	fn new_jobs(mut c: Context) -> Result<Vec<Self>> {
		if c.cue
			.discs
			.iter()
			.flat_map(|d| d.tracks.iter())
			.next()
			.is_none()
		{
			bail!("cuesheet has no tracks");
		}

		for d in &mut c.cue.discs {
			d.tracks.sort_unstable_by_key(|t| t.index);
		}

		let out_dir = c
			.args
			.out_dir
			.as_ref()
			.map_or_else(|| c.dir.join("split"), |p| p.clone());

		let mut md = cue_md(&c.cue);
		let md_trunc = md.len();

		let track_number_width = c
			.cue
			.discs
			.iter()
			.flat_map(|d| d.tracks.iter().map(|t| t.number))
			.max()
			.unwrap_or(1)
			.ilog10() as usize
			+ 1;

		let mut jobs = Vec::with_capacity(c.cue.discs.len());
		// Lazily initialized inside the loop
		let mut dirname = None;

		for disc in &c.cue.discs {
			let mut new_files = Vec::with_capacity(disc.tracks.len());
			let mut cmd = Command::new(&c.args.ffmpeg);
			cmd.args(c.force_opt).args(["-loglevel", "error"]);

			md.truncate(md_trunc);
			push_disc_md(disc, &mut md);
			// Shadow md_trunc for this loop
			let md_trunc = md.len();

			let to_split = c.dir.join(&disc.file);
			cmd.arg("-i").arg(&to_split);
			// Used in template expansion
			const COPY_ARGS: &[&str] = &["-c", "copy"];
			let (ext, encode_args) = match c.args.preset {
				Some(p) => {
					let ext = p.ext();
					if !c.args.no_copy
						&& to_split
							.extension()
							.is_some_and(|e| e.eq_ignore_ascii_case(ext))
					{
						(ext, COPY_ARGS)
					} else {
						(ext, p.ffmpeg_args())
					}
				}
				None if c.args.encode_arg.is_empty() => try_copy_codec(&to_split)
					.map_or(("flac", Preset::Flac.ffmpeg_args()), |ext| (ext, COPY_ARGS)),
				None => {
					// At this point the user has specified --ext as well as some encode args
					(c.args.ext.as_deref().unwrap(), [].as_slice())
				}
			};

			let artist = disc
				.performer
				.as_deref()
				.or(c.cue.performer.as_deref())
				.map(normalize);
			let album = disc
				.title
				.as_deref()
				.or(c.cue.title.as_deref())
				.map(normalize);

			for (i, track) in disc.tracks.iter().enumerate() {
				let from = ms_to_ffmpeg(track.index);
				let to = disc.tracks.get(i + 1).map(|t| ms_to_ffmpeg(t.index));
				let title_in_file = track.title.as_deref().map(normalize);

				let out = c.args.template.expand(|buf, var| match var {
					"title" => buf.push(title_in_file.as_deref().unwrap_or("(untitled)")),
					"artist" => buf.push(
						track
							.performer
							.as_deref()
							.or(artist.as_deref())
							.unwrap_or("(unknown)"),
					),
					"album" => buf.push(album.as_deref().unwrap_or("(unknown)")),
					"year" => buf.push(&c.year),
					"no" => buf.push(format!(
						"{number:0track_number_width$}",
						number = track.number
					)),
					"dir-name" => buf.push(dirname.get_or_insert_with(|| {
						c.dir
							.canonicalize()
							.ok()
							.and_then(|p| p.file_name().map(|s| s.to_os_string()))
							.or_else(|| c.dir.file_name().map(|s| s.to_os_string()))
							.unwrap_or_default()
					})),
					"ext" => buf.push(ext),
					_ => unreachable!(),
				});

				let out = out_dir.join(out);

				md.truncate(md_trunc);
				push_track_md(track, &mut md);

				cmd.args(["-ss", &*from])
					.args(to.as_deref().into_iter().flat_map(|to| ["-to", to]))
					.args(md.iter().flat_map(|s| ["-metadata", s.as_str()]))
					.args(encode_args);

				if encode_args.is_empty() {
					cmd.args(&c.args.encode_arg);
				}
				cmd.arg(&out);

				new_files.push(out);
			}

			jobs.push(Self { cmd, new_files })
		}

		Ok(jobs)
	}

	fn run(mut self) -> Result<()> {
		for parent in self.new_files.iter().filter_map(|p| p.parent()) {
			fs::create_dir_all(parent)
				.map_err(|e| anyhow!("error creating directory {}: {}", parent.display(), e))?;
		}

		// println!("{:?}", self.cmd);
		let status = self.cmd.status()?;
		ensure!(status.success(), "ffmpeg exited with {status}");
		Ok(())
	}
}

fn main() {
	if let Err(e) = run() {
		eprintln!("error: {e:?}");
		std::process::exit(1);
	}
}
