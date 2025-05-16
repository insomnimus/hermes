#[derive(Copy, Clone, Debug, clap::ValueEnum)]
pub enum Preset {
	Wav,
	Flac,
	FlacComp10,
	LibopusLow,
	Libopus,
	LibopusHigh,
	LibopusUltra,
	Libmp3lameLow,
	Libmp3lame,
	Libmp3lameHigh,
	Libmp3lameUltra,
	LibfdkAacLow,
	LibfdkAac,
	LibfdkAacHigh,
	LibfdkAacUltra,
	LibvorbisLow,
	Libvorbis,
	LibvorbisHigh,
	LibvorbisUltra,
}

impl Preset {
	pub const fn ext(self) -> &'static str {
		use Preset::*;
		match self {
			Wav => "wav",
			Flac | FlacComp10 => "flac",
			Libopus | LibopusLow | LibopusHigh | LibopusUltra => "ogg",
			LibfdkAac | LibfdkAacLow | LibfdkAacHigh | LibfdkAacUltra => "m4a",
			Libmp3lame | Libmp3lameLow | Libmp3lameHigh | Libmp3lameUltra => "mp3",
			Libvorbis | LibvorbisLow | LibvorbisHigh | LibvorbisUltra => "ogg",
		}
	}

	pub const fn ffmpeg_args(self) -> &'static [&'static str] {
		use Preset::*;
		match self {
			Wav => &["-f", "wav"],
			Flac => &["-f", "flac", "-c:a", "flac", "-compression_level", "8"],
			FlacComp10 => &["-f", "flac", "-c:a", "flac", "-compression_level", "10"],

			LibopusLow => &["-f", "oga", "-c:a", "libopus", "-b:a", "48k"],
			Libopus => &["-f", "oga", "-c:a", "libopus", "-b:a", "128k"],
			LibopusHigh => &["-f", "oga", "-c:a", "libopus", "-b:a", "192k"],
			LibopusUltra => &["-f", "oga", "-c:a", "libopus", "-b:a", "256k"],

			Libmp3lameLow => &["-f", "mp3", "-c:a", "libmp3lame", "-b:a", "64k"],
			Libmp3lame => &["-f", "mp3", "-c:a", "libmp3lame", "-b:a", "128k"],
			Libmp3lameHigh => &["-f", "mp3", "-c:a", "libmp3lame", "-b:a", "224k"],
			Libmp3lameUltra => &["-f", "mp3", "-c:a", "libmp3lame", "-b:a", "320k"],

			LibfdkAacLow => &["-f", "mp4", "-c:a", "libfdk_aac", "-b:a", "64k"],
			LibfdkAac => &["-f", "mp4", "-c:a", "libfdk_aac", "-b:a", "128k"],
			LibfdkAacHigh => &["-f", "mp4", "-c:a", "libfdk_aac", "-b:a", "192k"],
			LibfdkAacUltra => &[
				"-f",
				"mp4",
				"-c:a",
				"libfdk_aac",
				"-b:a",
				"256k",
				"-cutoff",
				"18000",
			],

			LibvorbisLow => &["-f", "oga", "-c:a", "libvorbis", "-q", "2.0"],
			Libvorbis => &["-f", "oga", "-c:a", "libvorbis", "-q", "5.0"],
			LibvorbisHigh => &["-f", "oga", "-c:a", "libvorbis", "-q", "6.5"],
			LibvorbisUltra => &["-f", "oga", "-c:a", "libvorbis", "-q", "8.0"],
		}
	}
}
