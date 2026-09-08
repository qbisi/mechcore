use mechcore_mcfr::Rational;
use std::{
    fs::File,
    io::{Seek, SeekFrom, Write},
    path::{Path, PathBuf},
};
use tempfile::TempPath;

const JPEG_SAMPLE_ENTRY: &[u8; 4] = b"jpeg";

pub(crate) struct VideoSummary {
    pub(crate) frame_count: u64,
    pub(crate) width: u16,
    pub(crate) height: u16,
}

pub(crate) struct MovWriter {
    target: PathBuf,
    temporary: TempPath,
    file: File,
    mdat_start: u64,
    sample_offset: u64,
    sample_sizes: Vec<u32>,
    width: Option<u16>,
    height: Option<u16>,
    logic_step: Rational,
}

impl MovWriter {
    pub(crate) fn create(path: &Path, logic_step: Rational) -> Result<Self, String> {
        if path.exists() {
            return Err(format!("refusing to overwrite {}", path.display()));
        }
        if logic_step.numerator == 0 || logic_step.denominator == 0 {
            return Err("video logic step must be positive".into());
        }
        let parent = path.parent().unwrap_or_else(|| Path::new("."));
        std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        let temporary = tempfile::Builder::new()
            .prefix(".mechcore-video-")
            .suffix(".mov.part")
            .tempfile_in(parent)
            .map_err(|error| error.to_string())?
            .into_temp_path();
        let mut file = File::options()
            .read(true)
            .write(true)
            .open(&temporary)
            .map_err(|error| error.to_string())?;
        file.write_all(&atom(*b"ftyp", &ftyp_payload()))
            .map_err(|error| error.to_string())?;
        let mdat_start = file.stream_position().map_err(|error| error.to_string())?;
        file.write_all(&1_u32.to_be_bytes())
            .and_then(|()| file.write_all(b"mdat"))
            .and_then(|()| file.write_all(&0_u64.to_be_bytes()))
            .map_err(|error| error.to_string())?;
        let sample_offset = file.stream_position().map_err(|error| error.to_string())?;
        Ok(Self {
            target: path.to_path_buf(),
            temporary,
            file,
            mdat_start,
            sample_offset,
            sample_sizes: Vec::new(),
            width: None,
            height: None,
            logic_step,
        })
    }

    pub(crate) fn append_jpeg(&mut self, jpeg: &[u8]) -> Result<(), String> {
        let (width, height) = jpeg_dimensions(jpeg)?;
        match (self.width, self.height) {
            (None, None) => {
                self.width = Some(width);
                self.height = Some(height);
            }
            (Some(expected_width), Some(expected_height))
                if (expected_width, expected_height) != (width, height) =>
            {
                return Err(format!(
                    "video frame dimensions changed from {expected_width}x{expected_height} to {width}x{height}"
                ));
            }
            _ => {}
        }
        let size = u32::try_from(jpeg.len()).map_err(|_| "JPEG frame exceeds MOV limits")?;
        self.file
            .write_all(jpeg)
            .map_err(|error| error.to_string())?;
        self.sample_sizes.push(size);
        Ok(())
    }

    pub(crate) fn finish(mut self) -> Result<VideoSummary, String> {
        if self.sample_sizes.is_empty() {
            return Err("cannot publish a video without frames".into());
        }
        let width = self.width.expect("first frame sets width");
        let height = self.height.expect("first frame sets height");
        let end_of_mdat = self
            .file
            .stream_position()
            .map_err(|error| error.to_string())?;
        let mdat_size = end_of_mdat
            .checked_sub(self.mdat_start)
            .ok_or("invalid MOV media-data offset")?;
        self.file
            .seek(SeekFrom::Start(self.mdat_start + 8))
            .and_then(|_| self.file.write_all(&mdat_size.to_be_bytes()))
            .and_then(|()| self.file.seek(SeekFrom::Start(end_of_mdat)).map(drop))
            .map_err(|error| error.to_string())?;
        let moov = moov(
            width,
            height,
            self.logic_step,
            self.sample_offset,
            &self.sample_sizes,
        )?;
        self.file
            .write_all(&moov)
            .and_then(|()| self.file.sync_all())
            .map_err(|error| error.to_string())?;
        drop(self.file);
        self.temporary
            .persist_noclobber(&self.target)
            .map_err(|error| error.error.to_string())?;
        Ok(VideoSummary {
            frame_count: u64::try_from(self.sample_sizes.len())
                .map_err(|_| "video frame count overflow")?,
            width,
            height,
        })
    }
}

fn jpeg_dimensions(jpeg: &[u8]) -> Result<(u16, u16), String> {
    if !jpeg.starts_with(&[0xff, 0xd8]) {
        return Err("visual capture did not return a JPEG image".into());
    }
    let mut offset = 2_usize;
    while offset + 4 <= jpeg.len() {
        if jpeg[offset] != 0xff {
            offset += 1;
            continue;
        }
        while offset < jpeg.len() && jpeg[offset] == 0xff {
            offset += 1;
        }
        let marker = *jpeg.get(offset).ok_or("truncated JPEG marker")?;
        offset += 1;
        if matches!(marker, 0x01 | 0xd0..=0xd9) {
            continue;
        }
        let length = u16::from_be_bytes([
            *jpeg.get(offset).ok_or("truncated JPEG segment")?,
            *jpeg.get(offset + 1).ok_or("truncated JPEG segment")?,
        ]) as usize;
        if length < 2 || offset + length > jpeg.len() {
            return Err("invalid JPEG segment length".into());
        }
        if matches!(marker, 0xc0..=0xc3 | 0xc5..=0xc7 | 0xc9..=0xcb | 0xcd..=0xcf) {
            if length < 7 {
                return Err("truncated JPEG dimensions".into());
            }
            let height = u16::from_be_bytes([jpeg[offset + 3], jpeg[offset + 4]]);
            let width = u16::from_be_bytes([jpeg[offset + 5], jpeg[offset + 6]]);
            if width == 0 || height == 0 {
                return Err("JPEG dimensions must be positive".into());
            }
            return Ok((width, height));
        }
        offset += length;
    }
    Err("JPEG image has no supported start-of-frame marker".into())
}

fn ftyp_payload() -> Vec<u8> {
    let mut payload = Vec::with_capacity(12);
    payload.extend_from_slice(b"qt  ");
    payload.extend_from_slice(&0x0000_0200_u32.to_be_bytes());
    payload.extend_from_slice(b"qt  ");
    payload
}

fn moov(
    width: u16,
    height: u16,
    logic_step: Rational,
    sample_offset: u64,
    sample_sizes: &[u32],
) -> Result<Vec<u8>, String> {
    let count = u32::try_from(sample_sizes.len()).map_err(|_| "video has too many frames")?;
    let timescale = logic_step.denominator;
    let sample_duration = logic_step.numerator;
    let duration_u64 = u64::from(count)
        .checked_mul(u64::from(sample_duration))
        .ok_or("video duration overflow")?;
    let duration =
        u32::try_from(duration_u64).map_err(|_| "video duration exceeds MOV v0 limits")?;

    let movie_header = atom(*b"mvhd", &mvhd_payload(timescale, duration));
    let track_header = atom(*b"tkhd", &tkhd_payload(duration, width, height));
    let media_header = atom(*b"mdhd", &mdhd_payload(timescale, duration));
    let handler = atom(*b"hdlr", &hdlr_payload());
    let video_header = atom(*b"vmhd", &vmhd_payload());
    let data_information = atom(*b"dinf", &dinf_payload());
    let sample_description = atom(*b"stsd", &stsd_payload(width, height));
    let sample_times = atom(*b"stts", &stts_payload(count, sample_duration));
    let sample_chunks = atom(*b"stsc", &stsc_payload(count));
    let sample_sizes_atom = atom(*b"stsz", &stsz_payload(sample_sizes, count));
    let chunk_offsets = atom(*b"co64", &co64_payload(sample_offset));
    let sample_table = atom(
        *b"stbl",
        &[
            sample_description,
            sample_times,
            sample_chunks,
            sample_sizes_atom,
            chunk_offsets,
        ]
        .concat(),
    );
    let media_information = atom(
        *b"minf",
        &[video_header, data_information, sample_table].concat(),
    );
    let media = atom(
        *b"mdia",
        &[media_header, handler, media_information].concat(),
    );
    let track = atom(*b"trak", &[track_header, media].concat());
    Ok(atom(*b"moov", &[movie_header, track].concat()))
}

fn atom(kind: [u8; 4], payload: &[u8]) -> Vec<u8> {
    let size = u32::try_from(payload.len() + 8).expect("MOV atom exceeds u32");
    let mut value = Vec::with_capacity(payload.len() + 8);
    value.extend_from_slice(&size.to_be_bytes());
    value.extend_from_slice(&kind);
    value.extend_from_slice(payload);
    value
}

fn fullbox(version_and_flags: u32, body: &[u8]) -> Vec<u8> {
    let mut value = Vec::with_capacity(body.len() + 4);
    value.extend_from_slice(&version_and_flags.to_be_bytes());
    value.extend_from_slice(body);
    value
}

fn identity_matrix(value: &mut Vec<u8>) {
    for item in [0x0001_0000_u32, 0, 0, 0, 0x0001_0000, 0, 0, 0, 0x4000_0000] {
        value.extend_from_slice(&item.to_be_bytes());
    }
}

fn mvhd_payload(timescale: u32, duration: u32) -> Vec<u8> {
    let mut body = Vec::new();
    body.extend_from_slice(&0_u32.to_be_bytes());
    body.extend_from_slice(&0_u32.to_be_bytes());
    body.extend_from_slice(&timescale.to_be_bytes());
    body.extend_from_slice(&duration.to_be_bytes());
    body.extend_from_slice(&0x0001_0000_u32.to_be_bytes());
    body.extend_from_slice(&0x0100_u16.to_be_bytes());
    body.extend_from_slice(&[0; 10]);
    identity_matrix(&mut body);
    body.extend_from_slice(&[0; 24]);
    body.extend_from_slice(&2_u32.to_be_bytes());
    fullbox(0, &body)
}

fn tkhd_payload(duration: u32, width: u16, height: u16) -> Vec<u8> {
    let mut body = Vec::new();
    body.extend_from_slice(&0_u32.to_be_bytes());
    body.extend_from_slice(&0_u32.to_be_bytes());
    body.extend_from_slice(&1_u32.to_be_bytes());
    body.extend_from_slice(&0_u32.to_be_bytes());
    body.extend_from_slice(&duration.to_be_bytes());
    body.extend_from_slice(&[0; 8]);
    body.extend_from_slice(&0_i16.to_be_bytes());
    body.extend_from_slice(&0_i16.to_be_bytes());
    body.extend_from_slice(&0_i16.to_be_bytes());
    body.extend_from_slice(&0_u16.to_be_bytes());
    identity_matrix(&mut body);
    body.extend_from_slice(&(u32::from(width) << 16).to_be_bytes());
    body.extend_from_slice(&(u32::from(height) << 16).to_be_bytes());
    fullbox(0x0000_0003, &body)
}

fn mdhd_payload(timescale: u32, duration: u32) -> Vec<u8> {
    let mut body = Vec::new();
    body.extend_from_slice(&0_u32.to_be_bytes());
    body.extend_from_slice(&0_u32.to_be_bytes());
    body.extend_from_slice(&timescale.to_be_bytes());
    body.extend_from_slice(&duration.to_be_bytes());
    body.extend_from_slice(&0_u16.to_be_bytes());
    body.extend_from_slice(&0_u16.to_be_bytes());
    fullbox(0, &body)
}

fn hdlr_payload() -> Vec<u8> {
    let mut body = Vec::new();
    body.extend_from_slice(&0_u32.to_be_bytes());
    body.extend_from_slice(b"vide");
    body.extend_from_slice(&[0; 12]);
    body.extend_from_slice(b"Mechcore logic frames\0");
    fullbox(0, &body)
}

fn vmhd_payload() -> Vec<u8> {
    fullbox(1, &[0; 8])
}

fn dinf_payload() -> Vec<u8> {
    let url = atom(*b"url ", &fullbox(1, &[]));
    let mut dref_body = Vec::new();
    dref_body.extend_from_slice(&1_u32.to_be_bytes());
    dref_body.extend_from_slice(&url);
    atom(*b"dref", &fullbox(0, &dref_body))
}

fn stsd_payload(width: u16, height: u16) -> Vec<u8> {
    let mut entry = Vec::new();
    entry.extend_from_slice(&[0; 6]);
    entry.extend_from_slice(&1_u16.to_be_bytes());
    entry.extend_from_slice(&0_u16.to_be_bytes());
    entry.extend_from_slice(&0_u16.to_be_bytes());
    entry.extend_from_slice(&0_u32.to_be_bytes());
    entry.extend_from_slice(&0_u32.to_be_bytes());
    entry.extend_from_slice(&0_u32.to_be_bytes());
    entry.extend_from_slice(&width.to_be_bytes());
    entry.extend_from_slice(&height.to_be_bytes());
    entry.extend_from_slice(&0x0048_0000_u32.to_be_bytes());
    entry.extend_from_slice(&0x0048_0000_u32.to_be_bytes());
    entry.extend_from_slice(&0_u32.to_be_bytes());
    entry.extend_from_slice(&1_u16.to_be_bytes());
    let mut compressor = [0_u8; 32];
    let name = b"Mechcore MJPEG";
    compressor[0] = u8::try_from(name.len()).expect("compressor name length");
    compressor[1..=name.len()].copy_from_slice(name);
    entry.extend_from_slice(&compressor);
    entry.extend_from_slice(&24_u16.to_be_bytes());
    entry.extend_from_slice(&(-1_i16).to_be_bytes());
    let sample_entry = atom(*JPEG_SAMPLE_ENTRY, &entry);
    let mut body = Vec::new();
    body.extend_from_slice(&1_u32.to_be_bytes());
    body.extend_from_slice(&sample_entry);
    fullbox(0, &body)
}

fn stts_payload(count: u32, duration: u32) -> Vec<u8> {
    let mut body = Vec::new();
    body.extend_from_slice(&1_u32.to_be_bytes());
    body.extend_from_slice(&count.to_be_bytes());
    body.extend_from_slice(&duration.to_be_bytes());
    fullbox(0, &body)
}

fn stsc_payload(count: u32) -> Vec<u8> {
    let mut body = Vec::new();
    body.extend_from_slice(&1_u32.to_be_bytes());
    body.extend_from_slice(&1_u32.to_be_bytes());
    body.extend_from_slice(&count.to_be_bytes());
    body.extend_from_slice(&1_u32.to_be_bytes());
    fullbox(0, &body)
}

fn stsz_payload(sizes: &[u32], count: u32) -> Vec<u8> {
    let mut body = Vec::new();
    body.extend_from_slice(&0_u32.to_be_bytes());
    body.extend_from_slice(&count.to_be_bytes());
    for size in sizes {
        body.extend_from_slice(&size.to_be_bytes());
    }
    fullbox(0, &body)
}

fn co64_payload(offset: u64) -> Vec<u8> {
    let mut body = Vec::new();
    body.extend_from_slice(&1_u32.to_be_bytes());
    body.extend_from_slice(&offset.to_be_bytes());
    fullbox(0, &body)
}

#[cfg(test)]
mod tests {
    use super::*;

    const ONE_BY_ONE_JPEG: &[u8] = &[
        0xff, 0xd8, 0xff, 0xc0, 0x00, 0x0b, 0x08, 0x00, 0x01, 0x00, 0x01, 0x01, 0x01, 0x11, 0x00,
        0xff, 0xd9,
    ];

    #[test]
    fn reads_jpeg_dimensions() {
        assert_eq!(jpeg_dimensions(ONE_BY_ONE_JPEG).unwrap(), (1, 1));
    }

    #[test]
    fn writes_atomic_quicktime_mjpeg_container() {
        let directory = tempfile::tempdir().unwrap();
        let output = directory.path().join("battle.mov");
        let mut writer = MovWriter::create(
            &output,
            Rational {
                numerator: 1,
                denominator: 20,
            },
        )
        .unwrap();
        writer.append_jpeg(ONE_BY_ONE_JPEG).unwrap();
        writer.append_jpeg(ONE_BY_ONE_JPEG).unwrap();
        let summary = writer.finish().unwrap();
        assert_eq!(summary.frame_count, 2);
        assert_eq!((summary.width, summary.height), (1, 1));
        let bytes = std::fs::read(output).unwrap();
        assert!(bytes.windows(4).any(|window| window == b"ftyp"));
        assert!(bytes.windows(4).any(|window| window == b"mdat"));
        assert!(bytes.windows(4).any(|window| window == b"moov"));
        assert!(bytes.windows(4).any(|window| window == b"jpeg"));
    }

    #[test]
    fn refuses_dimension_changes() {
        let directory = tempfile::tempdir().unwrap();
        let output = directory.path().join("battle.mov");
        let mut writer = MovWriter::create(
            &output,
            Rational {
                numerator: 1,
                denominator: 20,
            },
        )
        .unwrap();
        writer.append_jpeg(ONE_BY_ONE_JPEG).unwrap();
        let mut changed = ONE_BY_ONE_JPEG.to_vec();
        changed[10] = 2;
        assert!(writer.append_jpeg(&changed).is_err());
        assert!(!output.exists());
    }
}
