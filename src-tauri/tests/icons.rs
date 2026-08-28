use std::collections::BTreeSet;

use tauri::image::Image;

#[test]
fn application_pngs_have_expected_sizes_and_transparent_corners() {
    let assets: &[(&str, &[u8], u32)] = &[
        ("16x16.png", include_bytes!("../icons/16x16.png"), 16),
        ("20x20.png", include_bytes!("../icons/20x20.png"), 20),
        ("32x32.png", include_bytes!("../icons/32x32.png"), 32),
        ("48x48.png", include_bytes!("../icons/48x48.png"), 48),
        ("64x64.png", include_bytes!("../icons/64x64.png"), 64),
        ("128x128.png", include_bytes!("../icons/128x128.png"), 128),
        ("128x128@2x.png", include_bytes!("../icons/128x128@2x.png"), 256),
    ];
    for (name, bytes, size) in assets {
        let image = Image::from_bytes(bytes).unwrap_or_else(|error| panic!("{name}: {error}"));
        assert_eq!((image.width(), image.height()), (*size, *size), "{name}");
        let rgba = image.rgba();
        let last = (*size as usize * *size as usize - 1) * 4 + 3;
        assert_eq!(rgba[3], 0, "{name} top-left corner must be transparent");
        assert_eq!(rgba[last], 0, "{name} bottom-right corner must be transparent");
        assert!(rgba.chunks_exact(4).any(|pixel| pixel[3] == 255), "{name} must contain opaque artwork");
    }
}

#[test]
fn windows_icon_contains_all_required_frames() {
    let bytes = include_bytes!("../icons/icon.ico");
    assert_eq!(&bytes[0..4], &[0, 0, 1, 0]);
    let count = u16::from_le_bytes([bytes[4], bytes[5]]) as usize;
    let sizes: BTreeSet<_> = (0..count).map(|index| {
        let offset = 6 + index * 16;
        let width = if bytes[offset] == 0 { 256 } else { bytes[offset] as u16 };
        let height = if bytes[offset + 1] == 0 { 256 } else { bytes[offset + 1] as u16 };
        assert_eq!(width, height);
        width
    }).collect();
    assert_eq!(sizes, BTreeSet::from([16, 20, 24, 32, 40, 48, 64, 128, 256]));
}
