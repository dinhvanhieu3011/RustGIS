//! DWG, AutoCAD's own format, read through the free ODA File Converter when it is installed: the
//! drawing is converted once into an ASCII DXF in the cache folder, then read by the DXF reader
//! (blocks, layers, colours, text and hatches as a DXF's are). Nothing links to it: without the
//! program, a DWG falls back to GDAL, else reports how to get the converter.

use super::external::{self, fnv1a, output, stamp, touch};
use crate::{DataError, Result};
use std::path::{Path, PathBuf};
use std::process::Command;

/// Where to get the converter.
pub const DOWNLOAD: &str = "https://www.opendesign.com/guestfiles/oda_file_converter";

/// Whether a file name is a DWG.
pub fn is_dwg_name(name: &str) -> bool {
    Path::new(name).extension().and_then(|e| e.to_str()).is_some_and(|e| e.eq_ignore_ascii_case("dwg"))
}

/// The converter: from `RUSTGIS_ODA_BIN` (a folder), the `PATH`, else its install folder
/// (`Program Files\ODA\ODAFileConverter <version>`, the newest).
pub fn program() -> Option<PathBuf> {
    external::program("RUSTGIS_ODA_BIN", "ODAFileConverter").or_else(installed)
}

fn installed() -> Option<PathBuf> {
    let base = PathBuf::from(std::env::var_os("ProgramFiles")?).join("ODA");
    let mut dirs: Vec<PathBuf> = std::fs::read_dir(base)
        .ok()?
        .flatten()
        .take(1000)
        .filter(|e| e.file_name().to_string_lossy().starts_with("ODAFileConverter"))
        .map(|e| e.path().join("ODAFileConverter.exe"))
        .filter(|p| p.is_file())
        .collect();
    // ponytail: newest by name order ("ODAFileConverter 25.12.0"); compare versions if 9 → 10 bites.
    dirs.sort();
    dirs.pop()
}

/// Whether the converter is installed.
pub fn available() -> bool {
    program().is_some()
}

fn missing() -> DataError {
    DataError::Unsupported(format!(
        "reading DWG needs the free ODA File Converter: install it ({DOWNLOAD}), or put its folder in RUSTGIS_ODA_BIN, and add the file again"
    ))
}

/// The longest a conversion may run.
const TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10 * 60);

/// Conversions not used for this long are removed from the cache.
const KEEP: std::time::Duration = std::time::Duration::from_secs(30 * 24 * 3600);

/// Converts a DWG to an ASCII DXF in the cache, reusing an earlier conversion of the same file
/// (by its path, size and modification time). Returns the DXF's path.
pub fn to_dxf(src: &Path) -> Result<PathBuf> {
    let exe = program().ok_or_else(missing)?;
    let dir =
        external::cache_dir("RUSTGIS_ODA_CACHE", "oda").ok_or_else(|| DataError::Format("no cache folder for DWG conversions".into()))?;
    to_dxf_in(&exe, &dir, src)
}

fn to_dxf_in(exe: &Path, dir: &Path, src: &Path) -> Result<PathBuf> {
    std::fs::create_dir_all(dir)?;
    external::prune(dir, ours, KEEP);
    let full = std::path::absolute(src).unwrap_or_else(|_| src.to_path_buf());
    let (len, modified) = stamp(&full)?;
    let key = format!("{}\n{len}\n{modified}", full.display());
    let tag = format!("{:016x}", fnv1a(key.as_bytes()));
    let out = dir.join(format!("{tag}.dxf"));
    let keyfile = dir.join(format!("{tag}.key"));
    // The key is kept beside the conversion and checked, so a hash collision can't hand back
    // another drawing.
    if out.is_file() && std::fs::read_to_string(&keyfile).is_ok_and(|k| k == key) {
        touch(&out);
        return Ok(out);
    }
    // The converter takes folders: a private one in, a private one out.
    let work = dir.join(format!("{tag}.{}.work", std::process::id()));
    let _guard = WorkDir(work.clone());
    let (inp, outp) = (work.join("in"), work.join("out"));
    std::fs::create_dir_all(&inp)?;
    std::fs::create_dir_all(&outp)?;
    std::fs::copy(&full, inp.join("drawing.dwg"))?;
    let mut c = Command::new(exe);
    // In, out, version, type, recurse, audit (repairs what it can), filter.
    c.arg(&inp).arg(&outp).args(["ACAD2018", "DXF", "0", "1", "*.DWG"]);
    let o = output(c, TIMEOUT, "ODA File Converter")?;
    let made = outp.join("drawing.dxf");
    if !made.is_file() {
        let why =
            String::from_utf8_lossy(if o.stderr.is_empty() { &o.stdout } else { &o.stderr }).trim().chars().take(400).collect::<String>();
        return Err(DataError::Format(if why.is_empty() {
            "ODA File Converter couldn't convert the drawing".into()
        } else {
            format!("ODA File Converter: {why}")
        }));
    }
    let _ = std::fs::remove_file(&out);
    std::fs::rename(&made, &out)?;
    std::fs::write(&keyfile, &key)?;
    Ok(out)
}

/// A work folder removed when dropped.
struct WorkDir(PathBuf);

impl Drop for WorkDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Whether a file name is one this module writes: `<16 hex>.dxf|.key`.
fn ours(name: &str) -> bool {
    let Some((tag, rest)) = name.split_once('.') else { return false };
    tag.len() == 16 && tag.bytes().all(|b| b.is_ascii_hexdigit()) && matches!(rest, "dxf" | "key")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_and_failures() {
        assert!(is_dwg_name("a.DWG") && is_dwg_name("/x/b.dwg") && !is_dwg_name("a.dxf") && !is_dwg_name("dwg"));
        assert!(ours("0123456789abcdef.dxf") && ours("0123456789abcdef.key") && !ours("plan.dxf") && !ours("0123456789abcdef.dwg"));
        assert!(missing().to_string().contains(DOWNLOAD));
        // A converter that isn't there, or a drawing that isn't, is an error, not a panic.
        let dir = std::env::temp_dir().join(format!("rustgis-oda-test-{}", std::process::id()));
        let src = dir.join("in.dwg");
        assert!(to_dxf_in(Path::new("no-such-converter"), &dir, &src).is_err());
        std::fs::create_dir_all(&dir).ok();
        std::fs::write(&src, b"not a drawing").ok();
        assert!(to_dxf_in(&dir.join("no-such-converter.exe"), &dir, &src).is_err());
        // Nothing of the failed run is left behind but the drawing.
        let left: Vec<_> = std::fs::read_dir(&dir).map(|r| r.flatten().map(|e| e.file_name()).collect()).unwrap_or_default();
        assert_eq!(left, vec![std::ffi::OsString::from("in.dwg")]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A real conversion, when the converter is installed and `RUSTGIS_ODA_SAMPLE` names a DWG.
    #[test]
    fn converts_when_installed() {
        let (Some(_), Some(sample)) = (program(), std::env::var_os("RUSTGIS_ODA_SAMPLE")) else { return };
        let dxf = match to_dxf(Path::new(&sample)) {
            Ok(d) => d,
            Err(e) => panic!("{e}"),
        };
        let bytes = std::fs::read(&dxf).unwrap_or_default();
        let read = crate::dxf::read("sample.dxf", &bytes, &crate::dxf::Options::default());
        assert!(read.is_ok_and(|r| !r.is_empty()));
    }
}
