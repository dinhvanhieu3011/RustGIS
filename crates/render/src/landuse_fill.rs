//! Circular 12 land-use image fills ([`rustgis_map::tt12`]): each group's pattern image, read
//! once from the image folder and tiled over its parcels. Desktop only; on the web parcels keep
//! their colour.

use std::sync::Arc;

use tiny_skia::Pixmap;

#[cfg(not(target_arch = "wasm32"))]
/// Images bigger than this per side are refused (a pattern tile is small).
const MAX_SIDE: u32 = 1024;

#[cfg(not(target_arch = "wasm32"))]
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

#[cfg(not(target_arch = "wasm32"))]
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
        let here = std::env::current_dir().ok().map(|d| d.join("img"));
        [chosen, env, exe, here].into_iter().flatten().find(|d| d.is_dir())
    }

    /// Decoded images, and when a missing or unreadable one was last tried.
    type Tiles = Mutex<HashMap<(PathBuf, String), Result<Arc<Pixmap>, web_time::Instant>>>;

    /// How long a missing or unreadable image waits before it is read again (one added while
    /// RustGIS runs shows then).
    const RETRY: std::time::Duration = std::time::Duration::from_secs(5);

    /// The most images kept (each at most 1024 × 1024).
    const MAX_TILES: usize = 128;

    /// `name`'s image in `dir`, decoded once.
    pub fn tile_in(dir: &std::path::Path, name: &str) -> Option<Arc<Pixmap>> {
        static TILES: OnceLock<Tiles> = OnceLock::new();
        if !super::plain(name) {
            return None;
        }
        let key = (dir.to_path_buf(), name.to_string());
        let tiles = TILES.get_or_init(Default::default);
        match tiles.lock().ok()?.get(&key) {
            Some(Ok(t)) => return Some(t.clone()),
            Some(Err(at)) if at.elapsed() < RETRY => return None,
            _ => {}
        }
        let t = std::fs::read(dir.join(name)).ok().and_then(|b| super::decode(&b)).map(Arc::new);
        let mut tiles = tiles.lock().ok()?;
        if tiles.len() >= MAX_TILES {
            tiles.clear();
        }
        tiles.insert(key, t.clone().ok_or_else(web_time::Instant::now));
        t
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub use folder::{image_dir, set_image_dir};

/// The image folder for one frame, looked for once.
pub(crate) struct Images(#[cfg(not(target_arch = "wasm32"))] Option<std::path::PathBuf>);

impl Images {
    pub(crate) fn find() -> Images {
        #[cfg(not(target_arch = "wasm32"))]
        {
            Images(folder::image_dir())
        }
        #[cfg(target_arch = "wasm32")]
        {
            Images()
        }
    }

    /// `name`'s pattern image, if the folder has it and it reads.
    pub(crate) fn tile(&self, name: &str) -> Option<Arc<Pixmap>> {
        #[cfg(not(target_arch = "wasm32"))]
        {
            folder::tile_in(self.0.as_deref()?, name)
        }
        #[cfg(target_arch = "wasm32")]
        {
            let _ = name;
            None
        }
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

    #[test]
    fn a_missing_image_is_read_again_later() {
        let dir = std::env::temp_dir().join(format!("rustgis-tt12-late-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        assert!(folder::tile_in(&dir, "late.png").is_none());
        std::fs::write(dir.join("late.png"), png(2, 2)).unwrap();
        assert!(folder::tile_in(&dir, "late.png").is_none(), "not read again at once");
        std::thread::sleep(std::time::Duration::from_millis(5100));
        assert!(folder::tile_in(&dir, "late.png").is_some(), "read once the wait is over");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
