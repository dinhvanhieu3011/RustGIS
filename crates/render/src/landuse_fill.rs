//! Circular 12 land-use image fills ([`rustgis_map::tt12`]): each group's pattern image, read
//! once from the image folder and tiled over its parcels. Desktop only; on the web parcels keep
//! their colour.

use std::sync::Arc;

use tiny_skia::Pixmap;

/// Images bigger than this per side are refused (a pattern tile is small).
const MAX_SIDE: u32 = 1024;

/// A pattern image's pixels from PNG bytes, premultiplied; `None` for bytes that aren't a small
/// image.
fn decode(bytes: &[u8]) -> Option<Pixmap> {
    let reader = || image::ImageReader::new(std::io::Cursor::new(bytes)).with_guessed_format().ok();
    let (w, h) = reader()?.into_dimensions().ok()?;
    if w == 0 || h == 0 || w > MAX_SIDE || h > MAX_SIDE {
        return None;
    }
    let rgba = reader()?.decode().ok()?.to_rgba8();
    let mut pm = Pixmap::new(w, h)?;
    for (dst, src) in pm.pixels_mut().iter_mut().zip(rgba.pixels()) {
        let [r, g, b, a] = src.0;
        *dst = tiny_skia::ColorU8::from_rgba(r, g, b, a).premultiply();
    }
    Some(pm)
}

/// A plain file name: no folders, nothing climbing out of the image folder.
fn plain(name: &str) -> bool {
    !name.is_empty() && !name.contains(['/', '\\', ':']) && name != ".." && name != "."
}

#[cfg(not(target_arch = "wasm32"))]
mod folder {
    use std::collections::HashMap;
    use std::path::PathBuf;
    use std::sync::{Arc, Mutex, OnceLock};

    use tiny_skia::Pixmap;

    static CHOSEN: Mutex<Option<PathBuf>> = Mutex::new(None);

    /// Sets the image folder, ahead of the ones looked for (`None` goes back to looking).
    pub fn set_image_dir(dir: Option<PathBuf>) {
        if let Ok(mut c) = CHOSEN.lock() {
            *c = dir;
        }
    }

    /// The image folder: the one set, else `RUSTGIS_TT12_IMG`, else `img` beside the program,
    /// else `img` in the working folder; the first that exists.
    pub fn image_dir() -> Option<PathBuf> {
        let chosen = CHOSEN.lock().ok().and_then(|c| c.clone());
        let env = std::env::var_os("RUSTGIS_TT12_IMG").map(PathBuf::from);
        let exe = std::env::current_exe().ok().and_then(|e| e.parent().map(|d| d.join("img")));
        [chosen, env, exe, Some(PathBuf::from("img"))].into_iter().flatten().find(|d| d.is_dir())
    }

    type Tiles = Mutex<HashMap<(PathBuf, String), Option<Arc<Pixmap>>>>;

    /// `name`'s image in `dir`, decoded once; a missing or unreadable one is remembered as none.
    pub fn tile_in(dir: &std::path::Path, name: &str) -> Option<Arc<Pixmap>> {
        static TILES: OnceLock<Tiles> = OnceLock::new();
        if !super::plain(name) {
            return None;
        }
        let key = (dir.to_path_buf(), name.to_string());
        let tiles = TILES.get_or_init(Default::default);
        if let Some(t) = tiles.lock().ok()?.get(&key) {
            return t.clone();
        }
        let t = std::fs::read(dir.join(name)).ok().and_then(|b| super::decode(&b)).map(Arc::new);
        tiles.lock().ok()?.insert(key, t.clone());
        t
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub use folder::{image_dir, set_image_dir};

/// `name`'s pattern image from the image folder, if there is one and it reads.
pub(crate) fn tile(name: &str) -> Option<Arc<Pixmap>> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        folder::tile_in(&folder::image_dir()?, name)
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = name;
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png(w: u32, h: u32) -> Vec<u8> {
        let img = image::RgbaImage::from_pixel(w, h, image::Rgba([255, 0, 0, 255]));
        let mut out = std::io::Cursor::new(Vec::new());
        img.write_to(&mut out, image::ImageFormat::Png).unwrap();
        out.into_inner()
    }

    #[test]
    fn tiles_read_once_and_refuse_bad_files() {
        let dir = std::env::temp_dir().join(format!("rustgis-tt12-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("t.png"), png(4, 4)).unwrap();
        std::fs::write(dir.join("bad.png"), [1u8]).unwrap();
        std::fs::write(dir.join("big.png"), png(2000, 2)).unwrap();
        let t = folder::tile_in(&dir, "t.png").unwrap();
        assert_eq!(t.width(), 4);
        // Cached: the same pixels, even once the file is gone.
        std::fs::remove_file(dir.join("t.png")).unwrap();
        assert!(Arc::ptr_eq(&t, &folder::tile_in(&dir, "t.png").unwrap()));
        for name in ["bad.png", "big.png", "missing.png", "../t.png", "a/t.png", "a\\t.png", ""] {
            assert!(folder::tile_in(&dir, name).is_none(), "{name}");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}
