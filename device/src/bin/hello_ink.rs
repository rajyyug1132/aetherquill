//! Phase-4 smoke test: prove qtfb connect + draw + input + clean exit
//! under AppLoad before the real app touches the device.
//! Draws "HELLO INK" + a border; any pen/touch press exits.

#[path = "../qtfb.rs"]
mod qtfb;

use qtfb::{QtfbClient, INPUT_PEN_PRESS, INPUT_TOUCH_PRESS, RM2_HEIGHT, RM2_WIDTH};

const BLACK: u16 = 0x0000;
const WHITE: u16 = 0xFFFF;

fn main() {
    let key: i32 = std::env::var("QTFB_KEY")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(245209899); // QTFB_DEFAULT_FRAMEBUFFER

    let mut client = match QtfbClient::connect(key) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("hello_ink: qtfb connect failed: {e}");
            std::process::exit(1);
        }
    };

    let px = client.pixels();
    px.fill(WHITE);

    // Border, 8px thick.
    for y in 0..RM2_HEIGHT {
        for x in 0..RM2_WIDTH {
            if x < 8 || x >= RM2_WIDTH - 8 || y < 8 || y >= RM2_HEIGHT - 8 {
                px[y * RM2_WIDTH + x] = BLACK;
            }
        }
    }

    // "HELLO INK" via font8x8, scaled 8x, centered.
    let text = "HELLO INK";
    let scale = 8;
    let text_w = text.len() * 8 * scale;
    let x0 = (RM2_WIDTH - text_w) / 2;
    let y0 = (RM2_HEIGHT - 8 * scale) / 2;
    for (ci, ch) in text.bytes().enumerate() {
        let glyph = font8x8::legacy::BASIC_LEGACY[ch as usize];
        for (row, bits) in glyph.iter().enumerate() {
            for col in 0..8 {
                if bits & (1 << col) != 0 {
                    for dy in 0..scale {
                        for dx in 0..scale {
                            let x = x0 + ci * 8 * scale + col * scale + dx;
                            let y = y0 + row * scale + dy;
                            px[y * RM2_WIDTH + x] = BLACK;
                        }
                    }
                }
            }
        }
    }

    if let Err(e) = client.update_all() {
        eprintln!("hello_ink: update failed: {e}");
        std::process::exit(1);
    }
    println!("hello_ink: drawn; tap screen to exit");

    // Exit on any press, or if the window is closed (drain_events errs).
    loop {
        match client.drain_events() {
            Ok(events) => {
                if events
                    .iter()
                    .any(|e| e.input_type == INPUT_PEN_PRESS || e.input_type == INPUT_TOUCH_PRESS)
                {
                    println!("hello_ink: input received, exiting clean");
                    return; // Drop sends MESSAGE_TERMINATE
                }
            }
            Err(_) => return,
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
}
