use astral_core::app::debug::card;
use astral_core::render::image::render;
use serde_json::json;

const PNG_SIGNATURE: &[u8] = b"\x89PNG\r\n\x1a\n";

fn width_of(png: &[u8]) -> u32 {
    u32::from_be_bytes(png[16..20].try_into().unwrap())
}

#[test]
fn renders_shapes_and_text_at_scale() {
    let spec = json!({
        "width": 120, "height": 40, "scale": 2, "background": 0xFF10_1010_u32,
        "items": [
            {"type": "rect", "x": 4, "y": 4, "w": 60, "h": 20, "radius": 10, "color": 0xFF58_65F2_u32, "border": 0xFFFF_FFFF_u32},
            {"type": "circle", "x": 100, "y": 20, "r": 6},
            {"type": "text", "x": 4, "y": 4, "text": "astral 你好", "width": 60, "weight": "bold", "align": "center"},
            {"type": "text", "x": 4, "y": 24, "text": "a very long line that truncates", "width": 40, "mono": true},
        ],
    });
    let png = render(&spec).expect("png");
    assert_eq!(&png[..8], PNG_SIGNATURE);
    assert_eq!(width_of(&png), 240);
    assert!(render(&json!({"width": 10})).is_none());
}

#[test]
fn renders_the_debug_card() {
    let environment = json!({
        "astral": "0.0.1", "commit": "e6cbc18", "discord": "357.0 (124376)", "discord_channel": "stable",
        "compatibility": "ready", "image": "astral.dylib", "framework": "ElleKit 1.1.3-2", "jailbreak": "Dopamine",
        "device_name": "iPhone SE (3rd generation)", "system": "iOS 18.5", "os_build": "22F76",
        "source": "App Store", "signer": "Apple", "install": "rootless", "markers": ["120 Hz", "Developer Mode on"],
        "binary": "encrypted · min 16.0 · arm64e · thinned for iPhone14,6 · PIE · hardened runtime off",
        "https": {"issuer": "Google Trust Services"}, "phoenix": "a1b2c3d4e5f6", "crashes_24h": 0,
        "plugin_list": [{"id": "a", "enabled": true}, {"id": "b", "enabled": false}],
    });
    let png = render(&card(&environment, "2026-11-26 16:00")).expect("card png");
    assert_eq!(&png[..8], PNG_SIGNATURE);
    if let Ok(path) = std::env::var("ASTRAL_RENDER_OUT") {
        std::fs::write(path, &png).unwrap();
    }
}
