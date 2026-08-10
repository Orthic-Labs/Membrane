fn main() {
    let manifest = std::path::PathBuf::from(
        std::env::var_os("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"),
    );
    let runtime_icon = manifest.join("icons").join("icon.png");
    std::fs::create_dir_all(runtime_icon.parent().expect("icon parent"))
        .expect("create icon directory");
    std::fs::write(&runtime_icon, PNG_1X1).expect("write generated runtime icon");
    let out = std::path::PathBuf::from(std::env::var_os("OUT_DIR").expect("OUT_DIR"));
    let icon = out.join("cortex.ico");
    std::fs::write(&icon, windows_icon()).expect("write generated icon");
    tauri_build::try_build(
        tauri_build::Attributes::new()
            .windows_attributes(tauri_build::WindowsAttributes::new().window_icon_path(icon)),
    )
    .expect("build Cortex tray")
}

const PNG_1X1: &[u8] = &[
    0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1f, 0x15, 0xc4,
    0x89, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x44, 0x41, 0x54, 0x78, 0xda, 0x63, 0x64, 0xf8, 0xcf, 0xf0,
    0x1f, 0x00, 0x05, 0xfe, 0x02, 0xfe, 0x0e, 0x7f, 0x27, 0x19, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45,
    0x4e, 0x44, 0xae, 0x42, 0x60, 0x82,
];

fn windows_icon() -> Vec<u8> {
    const WIDTH: u32 = 32;
    const PIXELS: u32 = WIDTH * WIDTH * 4;
    const MASK: u32 = WIDTH * WIDTH / 8;
    const IMAGE: u32 = 40 + PIXELS + MASK;
    let mut bytes = Vec::with_capacity((22 + IMAGE) as usize);
    bytes.extend_from_slice(&[0, 0, 1, 0, 1, 0]);
    bytes.extend_from_slice(&[32, 32, 0, 0, 1, 0, 32, 0]);
    bytes.extend_from_slice(&IMAGE.to_le_bytes());
    bytes.extend_from_slice(&22_u32.to_le_bytes());
    bytes.extend_from_slice(&40_u32.to_le_bytes());
    bytes.extend_from_slice(&(WIDTH as i32).to_le_bytes());
    bytes.extend_from_slice(&((WIDTH * 2) as i32).to_le_bytes());
    bytes.extend_from_slice(&1_u16.to_le_bytes());
    bytes.extend_from_slice(&32_u16.to_le_bytes());
    bytes.extend_from_slice(&0_u32.to_le_bytes());
    bytes.extend_from_slice(&PIXELS.to_le_bytes());
    bytes.extend_from_slice(&[0; 16]);
    for y in 0..WIDTH as i32 {
        for x in 0..WIDTH as i32 {
            let inside = (x - 16).pow(2) + (y - 16).pow(2) <= 132;
            bytes.extend_from_slice(if inside {
                &[136, 205, 63, 255]
            } else {
                &[20, 15, 11, 255]
            });
        }
    }
    bytes.extend(std::iter::repeat_n(0, MASK as usize));
    bytes
}
