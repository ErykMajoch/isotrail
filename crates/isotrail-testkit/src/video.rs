use std::path::Path;
use std::process::{Command, Stdio};

/// What ffprobe reports about a video's first stream
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VideoInfo {
    pub codec: String,
    pub pixel_format: String,
    pub color_space: String,
    pub width: u32,
    pub height: u32,
    pub frames: u32,
    pub frame_rate: String,
}

/// Whether `ffmpeg` and `ffprobe` are on PATH. Tests that need them pass
/// with a note when they aren't
pub fn has_ffmpeg() -> bool {
    let runs = |program: &str| {
        Command::new(program)
            .arg("-version")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|status| status.success())
    };
    let found = runs("ffmpeg") && runs("ffprobe");
    if !found {
        eprintln!("skipped: ffmpeg or ffprobe not found on PATH");
    }
    found
}

/// # Panics
///
/// If ffprobe can't read the video
pub fn probe_video(path: &Path) -> VideoInfo {
    let output = Command::new("ffprobe")
        .args(["-v", "error", "-select_streams", "v:0", "-count_frames"])
        .args(["-show_entries"])
        .arg("stream=codec_name,pix_fmt,color_space,width,height,nb_read_frames,r_frame_rate")
        .args(["-of", "default=noprint_wrappers=1"])
        .arg(path)
        .output()
        .expect("ffprobe runs");
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let field = |key: &str| {
        text.lines()
            .find_map(|line| line.strip_prefix(key)?.strip_prefix('='))
            .unwrap_or_else(|| panic!("ffprobe reports {key}: {text}"))
            .to_owned()
    };
    let number = |key: &str| field(key).parse().expect("a number");
    VideoInfo {
        codec: field("codec_name"),
        pixel_format: field("pix_fmt"),
        color_space: field("color_space"),
        width: number("width"),
        height: number("height"),
        frames: number("nb_read_frames"),
        frame_rate: field("r_frame_rate"),
    }
}

/// The first frame of a video as RGB rows, decoded by ffmpeg
///
/// # Panics
///
/// If ffmpeg can't decode the video
pub fn first_video_frame(path: &Path) -> Vec<u8> {
    let output = Command::new("ffmpeg")
        .args(["-v", "error", "-i"])
        .arg(path)
        .args(["-frames:v", "1", "-f", "rawvideo", "-pix_fmt", "rgb24", "-"])
        .stdin(Stdio::null())
        .output()
        .expect("ffmpeg runs");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    output.stdout
}
