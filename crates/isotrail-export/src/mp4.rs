use std::ffi::OsString;
use std::io::{self, Read, Write};
use std::path::{MAIN_SEPARATOR_STR, Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, SyncSender};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use isotrail_render::Frame;

use crate::{ExportError, upscale};

/// Longer than ffmpeg ever needs to take one frame or to finish the file
const PATIENCE: Duration = Duration::from_secs(60);

/// The ffmpeg program that encodes MP4 video
#[derive(Debug, Clone)]
pub struct Ffmpeg {
    program: PathBuf,
}

impl Ffmpeg {
    /// The `ffmpeg` on PATH
    pub fn find() -> Result<Ffmpeg, ExportError> {
        Ffmpeg::at("ffmpeg")
    }

    pub fn at(program: impl Into<PathBuf>) -> Result<Ffmpeg, ExportError> {
        let program = program.into();
        let status = Command::new(&program)
            .arg("-version")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
        match status {
            Ok(status) if status.success() => Ok(Ffmpeg { program }),
            Ok(_) => Err(ExportError::FfmpegMissing),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                Err(ExportError::FfmpegMissing)
            }
            Err(error) => Err(error.into()),
        }
    }
}

/// Streams frames, enlarged `scale` times, into ffmpeg, which encodes them as
/// H.264 in an MP4 file
pub struct Mp4Writer {
    child: Child,
    /// To the thread that writes into ffmpeg; dropped when the stream ends
    frames: Option<SyncSender<Vec<u8>>>,
    written: Receiver<io::Result<()>>,
    feeder: Option<JoinHandle<()>>,
    /// Frames handed to the feeder that it hasn't finished writing
    unwritten: usize,
    stderr: Option<JoinHandle<String>>,
    width: u32,
    height: u32,
    scale: u32,
    /// How long ffmpeg may go without taking a frame or finishing
    patience: Duration,
}

impl Mp4Writer {
    /// Frames are `width × height` internal pixels, shown `fps` times a second
    pub fn create(
        ffmpeg: &Ffmpeg,
        path: &Path,
        width: u32,
        height: u32,
        scale: u32,
        fps: u32,
    ) -> Result<Self, ExportError> {
        if scale == 0 {
            return Err(ExportError::ZeroScale);
        }
        let (out_width, out_height) = (width * scale, height * scale);
        if out_width == 0 || out_height == 0 || out_width % 2 == 1 || out_height % 2 == 1 {
            return Err(ExportError::Mp4Size {
                width: out_width,
                height: out_height,
            });
        }
        // A relative name starting with `-` would read as an option
        let mut output = OsString::new();
        if path.is_relative() {
            output.push(".");
            output.push(MAIN_SEPARATOR_STR);
        }
        output.push(path);
        let mut command = Command::new(&ffmpeg.program);
        command
            .args(["-hide_banner", "-loglevel", "error", "-y"])
            .args(["-f", "rawvideo", "-pix_fmt", "rgba"])
            .arg("-s")
            .arg(format!("{out_width}x{out_height}"))
            .arg("-framerate")
            .arg(fps.to_string())
            .args(["-i", "-", "-an"])
            .args(["-c:v", "libx264", "-tune", "animation", "-crf", "18"])
            // The animation tune doubles the reference frames, which pushes a
            // story past level 4.2; with four it is level 4.0 at 30 fps
            .args(["-refs", "4"])
            // Converted and tagged as bt709, so phones show the colours that
            // went in; the tags only reach the file when set in the filters
            .args(["-vf", "scale=out_color_matrix=bt709:out_range=tv:flags=accurate_rnd+full_chroma_int,format=yuv420p,setparams=color_primaries=bt709:color_trc=bt709:colorspace=bt709:range=tv"])
            .args(["-pix_fmt", "yuv420p", "-colorspace", "bt709"])
            .args(["-color_primaries", "bt709", "-color_trc", "bt709"])
            .args(["-movflags", "+faststart", "-f", "mp4"])
            .arg(output);
        Mp4Writer::spawn(command, (width, height), scale, PATIENCE)
    }

    fn spawn(
        mut command: Command,
        (width, height): (u32, u32),
        scale: u32,
        patience: Duration,
    ) -> Result<Self, ExportError> {
        let mut child = command
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|error| match error.kind() {
                io::ErrorKind::NotFound => ExportError::FfmpegMissing,
                _ => error.into(),
            })?;
        // Drained as it comes, so ffmpeg never waits on a full pipe
        let stderr = child.stderr.take().map(|mut pipe| {
            std::thread::spawn(move || {
                let mut bytes = Vec::new();
                let _ = pipe.read_to_end(&mut bytes);
                String::from_utf8_lossy(&bytes).into_owned()
            })
        });
        // Written from a thread of its own, so a stalled ffmpeg can be given up on
        let (frames, queued) = mpsc::sync_channel::<Vec<u8>>(1);
        let (report, written) = mpsc::channel();
        let feeder = child.stdin.take().map(|mut stdin| {
            std::thread::spawn(move || {
                for bytes in queued {
                    let result = stdin.write_all(&bytes);
                    let failed = result.is_err();
                    if report.send(result).is_err() || failed {
                        break;
                    }
                }
            })
        });
        Ok(Mp4Writer {
            child,
            frames: Some(frames),
            written,
            feeder,
            unwritten: 0,
            stderr,
            width,
            height,
            scale,
            patience,
        })
    }

    pub fn push(&mut self, frame: &Frame) -> Result<(), ExportError> {
        if (frame.width(), frame.height()) != (self.width, self.height) {
            return Err(ExportError::FrameSize {
                expected: (self.width, self.height),
                actual: (frame.width(), frame.height()),
            });
        }
        let big = upscale(frame, self.scale)?;
        // The previous frame was written while this one was rendered
        self.wait_for_writes()?;
        let sent = self
            .frames
            .as_ref()
            .is_some_and(|frames| frames.send(big.into_rgba()).is_ok());
        if !sent {
            return self.broken(io::ErrorKind::BrokenPipe.into());
        }
        self.unwritten += 1;
        Ok(())
    }

    /// Ends the stream and waits for ffmpeg to finish the file
    pub fn finish(mut self) -> Result<(), ExportError> {
        self.wait_for_writes()?;
        self.close()
    }

    fn wait_for_writes(&mut self) -> Result<(), ExportError> {
        while self.unwritten > 0 {
            match self.written.recv_timeout(self.patience) {
                Ok(Ok(())) => self.unwritten -= 1,
                // ffmpeg stopped reading, most likely because it failed
                Ok(Err(error)) => return self.broken(error),
                Err(RecvTimeoutError::Disconnected) => {
                    return self.broken(io::ErrorKind::BrokenPipe.into());
                }
                Err(RecvTimeoutError::Timeout) => return Err(self.stalled()),
            }
        }
        Ok(())
    }

    /// ffmpeg's own error if it failed, otherwise `error`
    fn broken(&mut self, error: io::Error) -> Result<(), ExportError> {
        self.close()?;
        Err(error.into())
    }

    fn close(&mut self) -> Result<(), ExportError> {
        self.unwritten = 0;
        drop(self.frames.take());
        if let Some(feeder) = self.feeder.take() {
            let _ = feeder.join();
        }
        let deadline = Instant::now() + self.patience;
        let status = loop {
            if let Some(status) = self.child.try_wait()? {
                break status;
            }
            if Instant::now() >= deadline {
                return Err(self.stalled());
            }
            std::thread::sleep(Duration::from_millis(20));
        };
        let stderr = self.take_stderr();
        if status.success() {
            Ok(())
        } else {
            Err(ExportError::Ffmpeg { status, stderr })
        }
    }

    fn stalled(&mut self) -> ExportError {
        self.stop();
        ExportError::FfmpegStalled {
            seconds: self.patience.as_secs(),
            stderr: self.take_stderr(),
        }
    }

    /// Ending ffmpeg also frees the feeder from a write that would never end
    fn stop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        drop(self.frames.take());
        if let Some(feeder) = self.feeder.take() {
            let _ = feeder.join();
        }
    }

    fn take_stderr(&mut self) -> String {
        self.stderr
            .take()
            .and_then(|thread| thread.join().ok())
            .unwrap_or_default()
            .trim()
            .to_owned()
    }
}

impl Drop for Mp4Writer {
    fn drop(&mut self) {
        if self.frames.is_some() || self.feeder.is_some() {
            self.stop();
        }
    }
}

#[cfg(test)]
mod tests {
    use isotrail_testkit::{first_video_frame, has_ffmpeg, probe_video};

    use super::*;

    const RED: [u8; 4] = [200, 30, 40, 255];

    #[test]
    fn frames_encode_as_story_ready_h264() {
        if !has_ffmpeg() {
            return;
        }
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("clip.mp4");
        let ffmpeg = Ffmpeg::find().unwrap();
        let mut mp4 = Mp4Writer::create(&ffmpeg, &path, 16, 8, 2, 5).unwrap();
        for i in 0..10u8 {
            let mut rgba = RED.repeat(16 * 8);
            rgba[0] = i * 20;
            mp4.push(&Frame::new(16, 8, rgba)).unwrap();
        }
        mp4.finish().unwrap();

        let info = probe_video(&path);
        assert_eq!(info.codec, "h264");
        assert_eq!(info.pixel_format, "yuv420p");
        assert_eq!(info.color_space, "bt709");
        assert_eq!((info.width, info.height), (32, 16));
        assert_eq!(info.frames, 10);
        assert_eq!(info.frame_rate, "5/1");
        let rgb = first_video_frame(&path);
        assert_eq!(rgb.len(), 32 * 16 * 3);
        let centre = &rgb[(8 * 32 + 16) * 3..][..3];
        for (got, want) in centre.iter().zip(RED) {
            assert!(got.abs_diff(want) <= 4, "{centre:?} is not {RED:?}");
        }
    }

    /// Phones that refuse level 5.0 play it, and every colour field is set
    #[test]
    fn a_story_clip_is_level_4_and_fully_tagged() {
        if !has_ffmpeg() {
            return;
        }
        let temp = tempfile::tempdir().unwrap();
        let ffmpeg = Ffmpeg::find().unwrap();
        for (fps, most) in [(30, 40), (50, 42)] {
            let path = temp.path().join(format!("story-{fps}.mp4"));
            let mut mp4 = Mp4Writer::create(&ffmpeg, &path, 360, 640, 3, fps).unwrap();
            for i in 0..6u8 {
                let rgba = [i * 40, 90, 160, 255].repeat(360 * 640);
                mp4.push(&Frame::new(360, 640, rgba)).unwrap();
            }
            mp4.finish().unwrap();
            let info = probe_video(&path);
            assert_eq!((info.width, info.height), (1080, 1920));
            assert!(info.level <= most, "level {} at {fps} fps", info.level);
            assert_eq!(info.color_space, "bt709");
            assert_eq!(info.color_transfer, "bt709");
            assert_eq!(info.color_primaries, "bt709");
            assert_eq!(info.color_range, "tv");
        }
    }

    #[test]
    fn a_missing_ffmpeg_is_a_clear_error() {
        let missing = Ffmpeg::at("isotrail-no-such-ffmpeg");
        assert!(matches!(missing, Err(ExportError::FfmpegMissing)));
        let message = ExportError::FfmpegMissing.to_string();
        assert!(message.contains("install"), "{message}");
    }

    #[test]
    fn bad_settings_are_errors() {
        if !has_ffmpeg() {
            return;
        }
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("clip.mp4");
        let ffmpeg = Ffmpeg::find().unwrap();
        let odd = Mp4Writer::create(&ffmpeg, &path, 15, 8, 1, 30);
        assert!(matches!(odd.err().unwrap(), ExportError::Mp4Size { .. }));
        let zero = Mp4Writer::create(&ffmpeg, &path, 16, 8, 0, 30);
        assert!(matches!(zero.err().unwrap(), ExportError::ZeroScale));
        let mut mp4 = Mp4Writer::create(&ffmpeg, &path, 16, 8, 1, 30).unwrap();
        let wrong = mp4.push(&Frame::new(1, 1, vec![0; 4]));
        assert!(matches!(wrong, Err(ExportError::FrameSize { .. })));
    }

    #[test]
    fn ffmpeg_failures_carry_its_message() {
        if !has_ffmpeg() {
            return;
        }
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("missing").join("clip.mp4");
        let ffmpeg = Ffmpeg::find().unwrap();
        let mut mp4 = Mp4Writer::create(&ffmpeg, &path, 16, 8, 1, 30).unwrap();
        let frame = Frame::new(16, 8, RED.repeat(16 * 8));
        let pushed = (0..100).find_map(|_| mp4.push(&frame).err());
        let error = pushed.or_else(|| mp4.finish().err()).unwrap();
        assert!(
            matches!(&error, ExportError::Ffmpeg { stderr, .. } if !stderr.is_empty()),
            "{error:?}"
        );
    }

    /// A program that keeps running and never reads its input
    fn deaf() -> Command {
        if cfg!(windows) {
            let mut command = Command::new("ping");
            command.args(["-n", "30", "127.0.0.1"]);
            command
        } else {
            let mut command = Command::new("sleep");
            command.arg("30");
            command
        }
    }

    #[test]
    fn a_stalled_ffmpeg_is_given_up_on() {
        let patience = Duration::from_millis(500);
        let Ok(mut mp4) = Mp4Writer::spawn(deaf(), (512, 512), 1, patience) else {
            eprintln!("skipped: no program to stand in for a stalled ffmpeg");
            return;
        };
        let started = Instant::now();
        let frame = Frame::new(512, 512, RED.repeat(512 * 512));
        let error = (0..10)
            .find_map(|_| mp4.push(&frame).err())
            .or_else(|| mp4.finish().err())
            .unwrap();
        assert!(
            matches!(error, ExportError::FfmpegStalled { .. }),
            "{error:?}"
        );
        assert!(started.elapsed() < Duration::from_secs(10));
        assert!(error.to_string().contains("stopped"), "{error}");
    }
}
