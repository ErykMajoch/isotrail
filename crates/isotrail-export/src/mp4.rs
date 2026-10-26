use std::ffi::OsString;
use std::io::{self, Read, Write};
use std::path::{MAIN_SEPARATOR_STR, Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::thread::JoinHandle;

use isotrail_render::Frame;

use crate::{ExportError, upscale};

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
    /// Taken when the stream ends
    stdin: Option<ChildStdin>,
    stderr: Option<JoinHandle<String>>,
    width: u32,
    height: u32,
    scale: u32,
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
        let mut child = Command::new(&ffmpeg.program)
            .args(["-hide_banner", "-loglevel", "error", "-y"])
            .args(["-f", "rawvideo", "-pix_fmt", "rgba"])
            .arg("-s")
            .arg(format!("{out_width}x{out_height}"))
            .arg("-framerate")
            .arg(fps.to_string())
            .args(["-i", "-", "-an"])
            .args(["-c:v", "libx264", "-tune", "animation", "-crf", "18"])
            // Converted and tagged as bt709, so phones show the colours that went in
            .args(["-vf", "scale=out_color_matrix=bt709:out_range=tv:flags=accurate_rnd+full_chroma_int,format=yuv420p"])
            .args(["-pix_fmt", "yuv420p", "-colorspace", "bt709"])
            .args(["-color_primaries", "bt709", "-color_trc", "bt709"])
            .args(["-movflags", "+faststart"])
            .arg(output)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|error| match error.kind() {
                io::ErrorKind::NotFound => ExportError::FfmpegMissing,
                _ => error.into(),
            })?;
        let stdin = child.stdin.take();
        // Drained as it comes, so ffmpeg never waits on a full pipe
        let stderr = child.stderr.take().map(|mut pipe| {
            std::thread::spawn(move || {
                let mut bytes = Vec::new();
                let _ = pipe.read_to_end(&mut bytes);
                String::from_utf8_lossy(&bytes).into_owned()
            })
        });
        Ok(Mp4Writer {
            child,
            stdin,
            stderr,
            width,
            height,
            scale,
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
        let written = match &mut self.stdin {
            Some(stdin) => stdin.write_all(big.rgba()),
            None => Err(io::ErrorKind::BrokenPipe.into()),
        };
        if let Err(error) = written {
            // ffmpeg stopped reading, most likely because it failed
            self.close()?;
            return Err(error.into());
        }
        Ok(())
    }

    /// Ends the stream and waits for ffmpeg to finish the file
    pub fn finish(mut self) -> Result<(), ExportError> {
        self.close()
    }

    fn close(&mut self) -> Result<(), ExportError> {
        drop(self.stdin.take());
        let status = self.child.wait()?;
        let stderr = self
            .stderr
            .take()
            .and_then(|thread| thread.join().ok())
            .unwrap_or_default();
        if status.success() {
            Ok(())
        } else {
            Err(ExportError::Ffmpeg {
                status,
                stderr: stderr.trim().to_owned(),
            })
        }
    }
}

impl Drop for Mp4Writer {
    fn drop(&mut self) {
        if self.stdin.is_some() {
            let _ = self.child.kill();
            let _ = self.child.wait();
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
}
