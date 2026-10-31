//! Compare two PNGs channel by channel, or report a picture's colour cast per brightness band.
//! See README.md.

use std::ffi::OsString;
use std::fs::File;
use std::io::BufReader;
use std::path::Path;
use std::process::ExitCode;

const USAGE: &str = "usage:
  png-compare diff A.png B.png   per-channel differences between two pictures of the same size
  png-compare cast A.png         colour cast of a picture, per brightness band

exit code: 0 done (diff: identical), 1 diff: the pictures differ, 2 trouble";

/// The largest decoded picture accepted. The png crate's own default (64 MiB) would refuse a
/// full-size photo: 24 MP of 8-bit RGB is already 72 MB.
const MAX_DECODED_BYTES: u64 = 4 << 30;

/// A picture reduced to 8-bit RGB, three values per pixel.
struct Picture {
    width: usize,
    height: usize,
    rgb: Vec<u8>,
}

fn load(path: &Path) -> Result<Picture, String> {
    let fail = |e: &dyn std::fmt::Display| format!("{}: {e}", path.display());
    let file = File::open(path).map_err(|e| fail(&e))?;
    let limits = png::Limits {
        bytes: usize::try_from(MAX_DECODED_BYTES).unwrap_or(usize::MAX),
    };
    let mut decoder = png::Decoder::new_with_limits(BufReader::new(file), limits);
    // palettes become RGB and depths under 8 bits become 8
    decoder.set_transformations(png::Transformations::EXPAND);
    let mut reader = decoder.read_info().map_err(|e| fail(&e))?;
    let size = reader
        .output_buffer_size()
        .ok_or_else(|| fail(&"too large to decode"))?;
    let mut buffer = vec![0; size];
    let info = reader.next_frame(&mut buffer).map_err(|e| fail(&e))?;
    let decoded = buffer
        .get(..info.buffer_size())
        .ok_or_else(|| fail(&"decoder returned a short frame"))?;
    let bytes_per_sample = if info.bit_depth == png::BitDepth::Sixteen {
        2
    } else {
        1
    };
    let samples = info.color_type.samples();
    let (width, height) = (info.width as usize, info.height as usize);
    let mut rgb = Vec::with_capacity(decoded.len() / bytes_per_sample / samples * 3);
    for pixel in decoded.chunks_exact(samples * bytes_per_sample) {
        // 16-bit samples are big endian, so the first byte of each is its 8-bit level
        let sample = |i: usize| pixel.get(i * bytes_per_sample).copied().unwrap_or(0);
        match samples {
            // grey, with or without alpha
            1 | 2 => rgb.extend_from_slice(&[sample(0); 3]),
            // RGB, with or without alpha
            _ => rgb.extend_from_slice(&[sample(0), sample(1), sample(2)]),
        }
    }
    Ok(Picture { width, height, rgb })
}

#[derive(Debug, PartialEq)]
struct Diff {
    identical: bool,
    mean: f64,
    largest: u8,
    /// Where `largest` is, as (x, y).
    largest_at: (usize, usize),
    over_1: u64,
    over_3: u64,
    values: u64,
}

fn compare(a: &Picture, b: &Picture) -> Result<Diff, String> {
    if (a.width, a.height) != (b.width, b.height) {
        return Err(format!(
            "sizes differ: {}x{} and {}x{}",
            a.width, a.height, b.width, b.height
        ));
    }
    let (mut sum, mut largest, mut over_1, mut over_3, mut largest_index) =
        (0u64, 0u8, 0u64, 0u64, 0usize);
    for (index, (x, y)) in a.rgb.iter().zip(&b.rgb).enumerate() {
        let d = x.abs_diff(*y);
        sum += u64::from(d);
        over_1 += u64::from(d > 1);
        over_3 += u64::from(d > 3);
        if d > largest {
            (largest, largest_index) = (d, index);
        }
    }
    let pixel = largest_index / 3;
    let width = a.width.max(1);
    Ok(Diff {
        identical: largest == 0,
        mean: sum as f64 / a.rgb.len().max(1) as f64,
        largest,
        largest_at: (pixel % width, pixel / width),
        over_1,
        over_3,
        values: a.rgb.len() as u64,
    })
}

/// Brightness bands, in 8-bit levels: finer in the shadows, where a cast is easiest to see.
const BANDS: [(f64, f64); 6] = [
    (0.0, 8.0),
    (8.0, 16.0),
    (16.0, 32.0),
    (32.0, 64.0),
    (64.0, 128.0),
    (128.0, 256.0),
];

#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Band {
    pixels: u64,
    /// Sum of each channel over the band's pixels.
    sum: [f64; 3],
}

impl Band {
    fn means(&self) -> [f64; 3] {
        self.sum.map(|v| v / self.pixels.max(1) as f64)
    }
    /// Above zero the band leans magenta, below zero it leans green.
    fn magenta(&self) -> f64 {
        let [r, g, b] = self.means();
        (r + b) / 2.0 - g
    }
}

fn cast(picture: &Picture) -> [Band; 6] {
    let mut bands = [Band::default(); 6];
    for pixel in picture.rgb.as_chunks::<3>().0 {
        let [r, g, b] = pixel.map(f64::from);
        let luma = 0.2126 * r + 0.7152 * g + 0.0722 * b;
        if let Some(band) = BANDS
            .iter()
            .position(|&(low, high)| luma >= low && luma < high)
            && let Some(band) = bands.get_mut(band)
        {
            band.pixels += 1;
            band.sum[0] += r;
            band.sum[1] += g;
            band.sum[2] += b;
        }
    }
    bands
}

fn print_diff(d: &Diff, a: &Picture) {
    println!("size             {}x{}", a.width, a.height);
    println!(
        "identical        {}",
        if d.identical { "yes" } else { "no" }
    );
    println!("mean difference  {:.4} levels", d.mean);
    println!(
        "largest          {} levels, at pixel ({}, {})",
        d.largest, d.largest_at.0, d.largest_at.1
    );
    println!("off by over 1    {} of {} values", d.over_1, d.values);
    println!("off by over 3    {} of {} values", d.over_3, d.values);
}

fn print_cast(picture: &Picture) {
    println!("size {}x{}", picture.width, picture.height);
    println!("band      share     mean R  mean G  mean B  magenta");
    let total = (picture.width * picture.height).max(1) as f64;
    for (&(low, high), band) in BANDS.iter().zip(cast(picture).iter()) {
        if band.pixels == 0 {
            continue;
        }
        let [r, g, b] = band.means();
        println!(
            "{low:>3.0}-{high:<3.0}  {:>6.2} %   {r:>6.2}  {g:>6.2}  {b:>6.2}  {:>+7.2}",
            band.pixels as f64 * 100.0 / total,
            band.magenta()
        );
    }
}

fn run(args: &[OsString]) -> Result<ExitCode, String> {
    let command = args.first().and_then(|a| a.to_str());
    match (command, args.get(1), args.get(2), args.len()) {
        (Some("diff"), Some(a), Some(b), 3) => {
            let (a, b) = (load(Path::new(a))?, load(Path::new(b))?);
            let d = compare(&a, &b)?;
            print_diff(&d, &a);
            Ok(if d.identical {
                ExitCode::SUCCESS
            } else {
                ExitCode::from(1)
            })
        }
        (Some("cast"), Some(a), None, 2) => {
            print_cast(&load(Path::new(a))?);
            Ok(ExitCode::SUCCESS)
        }
        (Some("--help" | "-h" | "help"), None, None, 1) => {
            println!("{USAGE}");
            Ok(ExitCode::SUCCESS)
        }
        _ => Err(USAGE.to_string()),
    }
}

fn main() -> ExitCode {
    let args: Vec<OsString> = std::env::args_os().skip(1).collect();
    run(&args).unwrap_or_else(|message| {
        eprintln!("{message}");
        ExitCode::from(2)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::BufWriter;
    use std::path::PathBuf;

    /// A file in a fresh folder of this test's own; `name` keeps parallel tests apart.
    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("png-compare-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir.join("picture.png")
    }

    fn write(
        path: &Path,
        (w, h): (u32, u32),
        color: png::ColorType,
        depth: png::BitDepth,
        palette: Option<Vec<u8>>,
        data: &[u8],
    ) {
        let mut encoder = png::Encoder::new(BufWriter::new(File::create(path).unwrap()), w, h);
        encoder.set_color(color);
        encoder.set_depth(depth);
        if let Some(p) = palette {
            encoder.set_palette(p);
        }
        let mut writer = encoder.write_header().unwrap();
        writer.write_image_data(data).unwrap();
        writer.finish().unwrap();
    }

    fn picture(width: usize, height: usize, rgb: Vec<u8>) -> Picture {
        Picture { width, height, rgb }
    }

    #[test]
    fn identical_pictures_report_no_difference() {
        let a = picture(2, 1, vec![1, 2, 3, 4, 5, 6]);
        let d = compare(&a, &a).unwrap();
        assert!(d.identical);
        assert_eq!(
            (d.mean, d.largest, d.over_1, d.over_3, d.values),
            (0.0, 0, 0, 0, 6)
        );
    }

    #[test]
    fn differences_are_counted_and_located() {
        let a = picture(2, 2, vec![10; 12]);
        let mut changed = vec![10; 12];
        changed[11] = 50; // blue of the last pixel, (1, 1): 40 levels
        changed[0] = 12; // red of the first: 2 levels
        let d = compare(&a, &picture(2, 2, changed)).unwrap();
        assert!(!d.identical);
        assert_eq!(
            (d.largest, d.largest_at, d.over_1, d.over_3),
            (40, (1, 1), 2, 1)
        );
        assert!((d.mean - 3.5).abs() < 1e-9, "{}", d.mean);
    }

    #[test]
    fn different_sizes_are_an_error() {
        assert!(
            compare(&picture(2, 1, vec![0; 6]), &picture(1, 2, vec![0; 6]))
                .unwrap_err()
                .contains("sizes differ")
        );
    }

    #[test]
    fn a_magenta_cast_lands_in_its_band_with_a_positive_sign() {
        // luma 0.2126*20 + 0.7152*5 + 0.0722*20 = 9.27: the 8-16 band
        let bands = cast(&picture(1, 1, vec![20, 5, 20]));
        assert_eq!(bands[1].pixels, 1);
        assert_eq!(bands[1].means(), [20.0, 5.0, 20.0]);
        assert_eq!(bands[1].magenta(), 15.0);
        assert!(
            bands
                .iter()
                .enumerate()
                .all(|(i, b)| i == 1 || b.pixels == 0)
        );
        // green the other way (luma 15.73, the same band), and white reaches the top band
        assert_eq!(cast(&picture(1, 1, vec![5, 20, 5]))[1].magenta(), -15.0);
        assert_eq!(cast(&picture(1, 1, vec![255; 3]))[5].pixels, 1);
    }

    #[test]
    fn rgb_and_rgba_load_without_their_alpha() {
        let path = temp("rgb");
        write(
            &path,
            (2, 1),
            png::ColorType::Rgb,
            png::BitDepth::Eight,
            None,
            &[1, 2, 3, 4, 5, 6],
        );
        assert_eq!(load(&path).unwrap().rgb, [1, 2, 3, 4, 5, 6]);
        write(
            &path,
            (1, 1),
            png::ColorType::Rgba,
            png::BitDepth::Eight,
            None,
            &[7, 8, 9, 200],
        );
        assert_eq!(load(&path).unwrap().rgb, [7, 8, 9]);
    }

    #[test]
    fn grey_and_palette_load_as_rgb() {
        let path = temp("grey");
        write(
            &path,
            (2, 1),
            png::ColorType::Grayscale,
            png::BitDepth::Eight,
            None,
            &[9, 200],
        );
        assert_eq!(load(&path).unwrap().rgb, [9, 9, 9, 200, 200, 200]);
        write(
            &path,
            (1, 1),
            png::ColorType::GrayscaleAlpha,
            png::BitDepth::Eight,
            None,
            &[70, 30],
        );
        assert_eq!(load(&path).unwrap().rgb, [70, 70, 70]);
        write(
            &path,
            (2, 1),
            png::ColorType::Indexed,
            png::BitDepth::Eight,
            Some(vec![1, 2, 3, 40, 50, 60]),
            &[1, 0],
        );
        assert_eq!(load(&path).unwrap().rgb, [40, 50, 60, 1, 2, 3]);
    }

    #[test]
    fn sixteen_bit_keeps_the_high_byte() {
        let path = temp("deep");
        write(
            &path,
            (1, 1),
            png::ColorType::Rgb,
            png::BitDepth::Sixteen,
            None,
            &[0x12, 0xff, 0x34, 0x00, 0x56, 0x80],
        );
        assert_eq!(load(&path).unwrap().rgb, [0x12, 0x34, 0x56]);
        write(
            &path,
            (1, 1),
            png::ColorType::Grayscale,
            png::BitDepth::Sixteen,
            None,
            &[0xab, 0xcd],
        );
        assert_eq!(load(&path).unwrap().rgb, [0xab, 0xab, 0xab]);
    }

    /// 75 MB decoded: over the png crate's default 64 MiB limit, which would refuse a full-size photo.
    #[test]
    fn a_picture_over_the_decoders_default_limit_loads() {
        let path = temp("large");
        write(
            &path,
            (5000, 5000),
            png::ColorType::Rgb,
            png::BitDepth::Eight,
            None,
            &vec![0; 75_000_000],
        );
        let p = load(&path).unwrap();
        assert_eq!((p.width, p.height, p.rgb.len()), (5000, 5000, 75_000_000));
    }

    #[test]
    fn bad_files_are_errors() {
        let path = temp("bad");
        assert!(load(&path).is_err(), "missing");
        std::fs::write(&path, b"not a png").unwrap();
        assert!(load(&path).is_err(), "not a png");
        write(
            &path,
            (64, 64),
            png::ColorType::Rgb,
            png::BitDepth::Eight,
            None,
            &[9; 64 * 64 * 3],
        );
        let whole = std::fs::read(&path).unwrap();
        std::fs::write(&path, &whole[..whole.len() / 2]).unwrap();
        assert!(load(&path).is_err(), "cut short");
    }

    #[test]
    fn a_wrong_command_line_prints_the_usage() {
        for args in [
            vec![],
            vec!["nonsense"],
            vec!["diff", "a.png"],
            vec!["cast"],
            vec!["cast", "a.png", "b.png"],
        ] {
            let args: Vec<OsString> = args.into_iter().map(OsString::from).collect();
            assert_eq!(run(&args).unwrap_err(), USAGE, "{args:?}");
        }
    }
}
