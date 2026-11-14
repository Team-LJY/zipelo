use super::*;

fn png_bytes(
    width: u32,
    height: u32,
    color: png::ColorType,
    depth: png::BitDepth,
    data: &[u8],
) -> Vec<u8> {
    let mut bytes = Vec::new();
    let mut encoder = png::Encoder::new(&mut bytes, width, height);
    encoder.set_color(color);
    encoder.set_depth(depth);
    encoder
        .write_header()
        .unwrap()
        .write_image_data(data)
        .unwrap();
    bytes
}
fn pixel() -> Vec<u8> {
    png_bytes(
        1,
        1,
        png::ColorType::Rgba,
        png::BitDepth::Eight,
        &[10, 20, 30, 40],
    )
}
fn chunk(kind: &[u8; 4], bytes: &[u8]) -> Vec<u8> {
    let mut result = (bytes.len() as u32).to_be_bytes().to_vec();
    result.extend(kind);
    result.extend(bytes);
    result.extend(crc32(0, &result[4..]).to_be_bytes());
    result
}
fn run(kind: Kind, bytes: Vec<u8>) -> Result<(PreviewInfo, Vec<u8>)> {
    decode(kind, 7, bytes, &mut || Ok(()))
}

#[test]
fn preview_text_checks_utf8_bom_and_source_boundaries() {
    for bytes in [
        Vec::new(),
        vec![b'x'; MAX_PREVIEW_TEXT_BYTES as usize],
        "한글\n".as_bytes().to_vec(),
        vec![0xef, 0xbb, 0xbf],
    ] {
        let (info, data) = run(Kind::Text, bytes.clone()).unwrap();
        assert!(info.valid());
        assert_eq!(info.entry_id, 7);
        assert_eq!(info.total_bytes, data.len() as u64);
        assert_eq!(info.format, PreviewFormat::Utf8Text);
        assert_eq!(
            data,
            bytes.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(&bytes)
        );
    }
    assert!(matches!(
        run(Kind::Text, vec![b'x'; MAX_PREVIEW_TEXT_BYTES as usize + 1]),
        Err(ErrorCode::Limit)
    ));
    assert!(matches!(
        run(Kind::Text, vec![0xff]),
        Err(ErrorCode::Integrity)
    ));
    assert!(matches!(
        run(Kind::Text, vec![0xef, 0xbb, 0xbf, 0xff]),
        Err(ErrorCode::Integrity)
    ));
}
#[test]
fn preview_extensions_are_explicit_and_ascii_case_insensitive() {
    for name in [
        "a.txt", "a.MD", "a.log", "a.csv", "a.JSON", "a.PnG", "a.jpg", "a.JPEG",
    ] {
        assert!(Kind::from_name(name).is_ok());
    }
    for name in ["a.svg", "a.gif", "a.webp", "a.exe", "a", "folder.txt/child"] {
        assert!(Kind::from_name(name).is_err());
    }
}
#[test]
fn preview_png_normalizes_gray_rgb_alpha_and_sixteen_bit_pixels() {
    for (color, depth, source, expected) in [
        (
            png::ColorType::Rgba,
            png::BitDepth::Eight,
            vec![10, 20, 30, 40],
            vec![10, 20, 30, 40],
        ),
        (
            png::ColorType::Rgb,
            png::BitDepth::Eight,
            vec![10, 20, 30],
            vec![10, 20, 30, 255],
        ),
        (
            png::ColorType::Grayscale,
            png::BitDepth::Eight,
            vec![10],
            vec![10, 10, 10, 255],
        ),
        (
            png::ColorType::GrayscaleAlpha,
            png::BitDepth::Eight,
            vec![10, 40],
            vec![10, 10, 10, 40],
        ),
        (
            png::ColorType::GrayscaleAlpha,
            png::BitDepth::Sixteen,
            vec![10, 99, 40, 99],
            vec![10, 10, 10, 40],
        ),
    ] {
        let (info, data) = run(Kind::Png, png_bytes(1, 1, color, depth, &source)).unwrap();
        assert_eq!((info.width, info.height, info.total_bytes), (1, 1, 4));
        assert_eq!(data, expected);
    }
}
#[test]
fn preview_png_rejects_crc_truncation_animation_and_format_mismatch() {
    let original = pixel();
    for length in [8, original.len() - 12, original.len() - 1] {
        assert!(run(Kind::Png, original[..length].to_vec()).is_err());
    }
    let mut bad = original.clone();
    bad[29] ^= 1;
    assert!(matches!(run(Kind::Png, bad), Err(ErrorCode::Integrity)));
    let mut trailing = original.clone();
    trailing.push(0);
    assert!(matches!(
        run(Kind::Png, trailing),
        Err(ErrorCode::Integrity)
    ));
    let mut animated = original.clone();
    animated.splice(33..33, chunk(b"acTL", &[0, 0, 0, 1, 0, 0, 0, 0]));
    assert!(matches!(
        run(Kind::Png, animated),
        Err(ErrorCode::UnsupportedFeature)
    ));
    assert!(matches!(
        run(Kind::Png, b"GIF89a".to_vec()),
        Err(ErrorCode::UnsupportedFormat)
    ));
}
#[test]
fn preview_png_checks_dimensions_and_pixels_before_decoding() {
    for (width, height) in [
        (8193u32, 1u32),
        (1, 8193),
        (4001, 4000),
        (8192, 8192),
        (u32::MAX, u32::MAX),
    ] {
        let mut bytes = pixel();
        bytes[16..20].copy_from_slice(&width.to_be_bytes());
        bytes[20..24].copy_from_slice(&height.to_be_bytes());
        let crc = crc32(0, &bytes[12..29]);
        bytes[29..33].copy_from_slice(&crc.to_be_bytes());
        assert!(
            matches!(run(Kind::Png, bytes), Err(ErrorCode::Limit)),
            "{width}x{height}"
        );
    }
    for (width, height) in [(8192, 1), (1, 8192), (4000, 4000)] {
        let bytes = png_bytes(
            width,
            height,
            png::ColorType::Rgba,
            png::BitDepth::Eight,
            &vec![0; (width * height * 4) as usize],
        );
        let (info, rgba) = run(Kind::Png, bytes).unwrap();
        assert_eq!((info.width, info.height), (width, height));
        assert_eq!(rgba.len(), (width * height * 4) as usize);
    }
}
#[test]
fn preview_image_source_limit_includes_ancillary_bytes() {
    let mut bytes = pixel();
    let at = bytes.len() - 12;
    let padding = vec![0; MAX_PREVIEW_IMAGE_BYTES as usize - bytes.len() - 12];
    bytes.splice(at..at, chunk(b"teSt", &padding));
    assert_eq!(bytes.len(), MAX_PREVIEW_IMAGE_BYTES as usize);
    assert!(run(Kind::Png, bytes.clone()).is_ok());
    bytes.push(0);
    assert!(matches!(run(Kind::Png, bytes), Err(ErrorCode::Limit)));
    assert!(matches!(
        run(Kind::Jpeg, vec![0; MAX_PREVIEW_IMAGE_BYTES as usize + 1]),
        Err(ErrorCode::Limit)
    ));
}
#[test]
fn preview_jpeg_strictly_decodes_baseline_and_progressive() {
    for bytes in [
        include_bytes!("fixtures/preview-baseline.jpg").as_slice(),
        include_bytes!("fixtures/preview-progressive.jpg").as_slice(),
    ] {
        let (info, data) = run(Kind::Jpeg, bytes.to_vec()).unwrap();
        assert_eq!((info.width, info.height, info.total_bytes), (3, 2, 24));
        for pixel in data.chunks_exact(4) {
            assert_eq!(pixel[3], 255);
            assert!(pixel[0] > 200 && pixel[1] < 60 && pixel[2] < 80);
        }
        for length in [2, bytes.len() / 2, bytes.len() - 1] {
            assert!(run(Kind::Jpeg, bytes[..length].to_vec()).is_err());
        }
        let mut incomplete = bytes[..bytes.len() / 2].to_vec();
        incomplete.extend([0xff, 0xd9]);
        assert!(run(Kind::Jpeg, incomplete).is_err());
    }
    assert!(matches!(
        run(Kind::Jpeg, pixel()),
        Err(ErrorCode::UnsupportedFormat)
    ));
}
#[test]
fn preview_cancellation_discards_decoded_results_and_conversion() {
    for kind in [Kind::Text, Kind::Png, Kind::Jpeg] {
        let source = match kind {
            Kind::Text => b"text".to_vec(),
            Kind::Png => pixel(),
            Kind::Jpeg => include_bytes!("fixtures/preview-baseline.jpg").to_vec(),
        };
        let mut calls = 0;
        assert!(matches!(
            decode(kind, 7, source, &mut || {
                calls += 1;
                if calls >= 2 {
                    Err(ErrorCode::None)
                } else {
                    Ok(())
                }
            }),
            Err(ErrorCode::None)
        ));
    }
}

#[test]
fn preview_jpeg_checks_dimensions_before_decoder_allocation() {
    let original = include_bytes!("fixtures/preview-baseline.jpg");
    let at = original.windows(2).position(|p| p == [0xff, 0xc0]).unwrap();
    for (width, height) in [(8193u16, 1u16), (1, 8193), (4001, 4000), (8192, 8192)] {
        let mut bytes = original.to_vec();
        bytes[at + 5..at + 7].copy_from_slice(&height.to_be_bytes());
        bytes[at + 7..at + 9].copy_from_slice(&width.to_be_bytes());
        assert!(
            matches!(run(Kind::Jpeg, bytes), Err(ErrorCode::Limit)),
            "{width}x{height}"
        );
    }
    let mut multiple = original.to_vec();
    multiple.extend(original);
    assert!(run(Kind::Jpeg, multiple).is_err());
}

#[test]
fn preview_jpeg_normalizes_gray_and_cmyk_to_rgba() {
    for (bytes, gray) in [
        (include_bytes!("fixtures/preview-gray.jpg").as_slice(), true),
        (
            include_bytes!("fixtures/preview-cmyk.jpg").as_slice(),
            false,
        ),
    ] {
        let (info, pixels) = run(Kind::Jpeg, bytes.to_vec()).unwrap();
        assert_eq!((info.width, info.height, info.total_bytes), (3, 2, 24));
        for pixel in pixels.chunks_exact(4) {
            assert_eq!(pixel[3], 255);
            if gray {
                assert!(pixel[..3].iter().all(|v| v.abs_diff(80) <= 2));
            } else {
                assert!(pixel[0] > 240 && pixel[1] < 20 && pixel[2] < 20);
            }
        }
    }
}

#[test]
fn preview_jpeg_rejects_missing_scan_pixels() {
    let original = include_bytes!("fixtures/preview-baseline.jpg");
    let mut missing = original[..623].to_vec();
    missing.extend([0xff, 0xd9]);
    assert!(matches!(
        run(Kind::Jpeg, missing),
        Err(ErrorCode::Integrity)
    ));

    let mut missing_rows = original.to_vec();
    let sof = original.windows(2).position(|p| p == [0xff, 0xc0]).unwrap();
    missing_rows[sof + 5..sof + 7].copy_from_slice(&32u16.to_be_bytes());
    assert!(matches!(
        run(Kind::Jpeg, missing_rows),
        Err(ErrorCode::Integrity)
    ));

    let sos = original.windows(2).position(|p| p == [0xff, 0xda]).unwrap();
    let mut missing_components = original.to_vec();
    missing_components[sos + 2..sos + 4].copy_from_slice(&8u16.to_be_bytes());
    missing_components[sos + 4] = 1;
    missing_components.drain(sos + 7..sos + 11);
    assert!(matches!(
        jpeg_info(7, &missing_components, &mut || Ok(())),
        Err(ErrorCode::Integrity)
    ));
}

#[test]
fn preview_png_rejects_complete_container_with_missing_pixel_rows() {
    let mut missing = pixel();
    missing[20..24].copy_from_slice(&2u32.to_be_bytes());
    let crc = crc32(0, &missing[12..29]);
    missing[29..33].copy_from_slice(&crc.to_be_bytes());
    assert!(matches!(run(Kind::Png, missing), Err(ErrorCode::Integrity)));
}

#[test]
fn preview_jpeg_validates_multiline_scans_and_restart_boundaries() {
    for bytes in [
        include_bytes!("fixtures/preview-rows-baseline.jpg").as_slice(),
        include_bytes!("fixtures/preview-rows-progressive.jpg").as_slice(),
        include_bytes!("fixtures/preview-rows-gray.jpg").as_slice(),
        include_bytes!("fixtures/preview-rows-cmyk.jpg").as_slice(),
        include_bytes!("fixtures/preview-rows-restart.jpg").as_slice(),
    ] {
        let (info, pixels) = run(Kind::Jpeg, bytes.to_vec()).unwrap();
        assert_eq!((info.width, info.height), (31, 33));
        assert_eq!(pixels.len(), 31 * 33 * 4);
        assert!(pixels.chunks_exact(4).all(|pixel| pixel[3] == 255));
        for (sos, _) in bytes
            .windows(2)
            .enumerate()
            .filter(|(_, p)| *p == [0xff, 0xda])
        {
            let scan_start =
                sos + 2 + usize::from(u16::from_be_bytes([bytes[sos + 2], bytes[sos + 3]]));
            let mut missing = bytes[..scan_start].to_vec();
            missing.extend([0xff, 0xd9]);
            assert!(
                matches!(run(Kind::Jpeg, missing), Err(ErrorCode::Integrity)),
                "empty scan={sos}"
            );
        }
        let sos = bytes.windows(2).position(|p| p == [0xff, 0xda]).unwrap();
        let scan_start =
            sos + 2 + usize::from(u16::from_be_bytes([bytes[sos + 2], bytes[sos + 3]]));
        for cut in [scan_start + 1, scan_start + 2] {
            let mut missing = bytes[..cut].to_vec();
            missing.extend([0xff, 0xd9]);
            assert!(
                matches!(run(Kind::Jpeg, missing), Err(ErrorCode::Integrity)),
                "cut={cut}"
            );
        }
    }
}
