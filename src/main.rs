use deskagent::app::EditorApp;
use eframe::egui;

fn main() -> eframe::Result<()> {
    // Capture full backtrace to file so we can diagnose panics without
    // needing an interactive terminal session.
    use std::sync::atomic::{AtomicU32, Ordering};
    static PANIC_COUNT: AtomicU32 = AtomicU32::new(0);
    std::panic::set_hook(Box::new(|info| {
        let n = PANIC_COUNT.fetch_add(1, Ordering::SeqCst);
        let bt = std::backtrace::Backtrace::force_capture();
        let msg = format!("=== PANIC #{n} ===\n{info}\n\nBacktrace:\n{bt}\n\n");
        eprintln!("{msg}");
        // Append so multiple panics are all captured
        use std::io::Write;
        if let Ok(mut f) = std::fs::OpenOptions::new()
            .create(true).append(true).open("/tmp/deskagent_panic.txt")
        {
            let _ = f.write_all(msg.as_bytes());
        }
    }));

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1280.0, 800.0])
            .with_min_inner_size([800.0, 500.0])
            .with_icon(one_icon()),
        ..Default::default()
    };

    eframe::run_native(
        "DeskAgent",
        options,
        Box::new(|cc| {
            // ── Load CJK fonts to fix Chinese/Japanese/Korean rendering ──────
            let mut fonts = egui::FontDefinitions::default();

            // Try common macOS / Linux / Windows CJK font paths in order.
            let cjk_candidates: &[&str] = &[
                // macOS
                "/System/Library/Fonts/PingFang.ttc",
                "/System/Library/Fonts/STHeiti Light.ttc",
                "/System/Library/Fonts/Hiragino Sans GB.ttc",
                // Linux
                "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
                "/usr/share/fonts/truetype/wqy/wqy-microhei.ttc",
                "/usr/share/fonts/truetype/arphic/uming.ttc",
                // Windows
                "C:/Windows/Fonts/msyh.ttc",
                "C:/Windows/Fonts/simsun.ttc",
            ];

            let mut loaded_cjk = false;
            for path in cjk_candidates {
                if let Ok(bytes) = std::fs::read(path) {
                    fonts.font_data.insert(
                        "cjk_font".to_owned(),
                        egui::FontData::from_owned(bytes),
                    );
                    // Append CJK font as fallback for both Proportional and Monospace
                    fonts
                        .families
                        .entry(egui::FontFamily::Proportional)
                        .or_default()
                        .push("cjk_font".to_owned());
                    fonts
                        .families
                        .entry(egui::FontFamily::Monospace)
                        .or_default()
                        .push("cjk_font".to_owned());
                    loaded_cjk = true;
                    break;
                }
            }

            if !loaded_cjk {
                eprintln!("[DeskAgent] Warning: no CJK font found; Chinese characters may render as boxes.");
            }

            cc.egui_ctx.set_fonts(fonts);

            Ok(Box::<EditorApp>::default())
        }),
    )
}

fn one_icon() -> egui::IconData {
    const W: u32 = 128;
    const H: u32 = 128;
    let mut rgba = vec![0_u8; (W * H * 4) as usize];

    for y in 0..H {
        for x in 0..W {
            let idx = ((y * W + x) * 4) as usize;
            rgba[idx] = 18;
            rgba[idx + 1] = 20;
            rgba[idx + 2] = 24;
            rgba[idx + 3] = 255;

            let border = x < 6 || x >= W - 6 || y < 6 || y >= H - 6;
            if border {
                rgba[idx] = 0;
                rgba[idx + 1] = 190;
                rgba[idx + 2] = 210;
            }
        }
    }

    draw_block_text(&mut rgba, W, 16, 44, "ONE");

    egui::IconData {
        rgba,
        width: W,
        height: H,
    }
}

fn draw_block_text(rgba: &mut [u8], width: u32, start_x: u32, start_y: u32, text: &str) {
    let mut x = start_x;
    for ch in text.chars() {
        draw_block_char(rgba, width, x, start_y, ch);
        x += 34;
    }
}

fn draw_block_char(rgba: &mut [u8], width: u32, x: u32, y: u32, ch: char) {
    const SCALE: u32 = 7;
    let rows: [&str; 7] = match ch {
        'O' => [
            "01110",
            "10001",
            "10001",
            "10001",
            "10001",
            "10001",
            "01110",
        ],
        'N' => [
            "10001",
            "11001",
            "10101",
            "10011",
            "10001",
            "10001",
            "10001",
        ],
        'E' => [
            "11111",
            "10000",
            "10000",
            "11110",
            "10000",
            "10000",
            "11111",
        ],
        _ => return,
    };

    for (row_idx, row) in rows.iter().enumerate() {
        for (col_idx, bit) in row.as_bytes().iter().enumerate() {
            if *bit != b'1' {
                continue;
            }
            fill_rect(
                rgba,
                width,
                x + col_idx as u32 * SCALE,
                y + row_idx as u32 * SCALE,
                SCALE - 1,
                SCALE - 1,
                [235, 245, 248, 255],
            );
        }
    }
}

fn fill_rect(
    rgba: &mut [u8],
    width: u32,
    x: u32,
    y: u32,
    w: u32,
    h: u32,
    color: [u8; 4],
) {
    let height = (rgba.len() as u32 / 4) / width;
    for yy in y..(y + h).min(height) {
        for xx in x..(x + w).min(width) {
            let idx = ((yy * width + xx) * 4) as usize;
            rgba[idx..idx + 4].copy_from_slice(&color);
        }
    }
}
