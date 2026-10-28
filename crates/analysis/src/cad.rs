//! Digitising CAD drawings: tools over the layers a DXF or DWG drawing is read into
//! (`rustgis_data::dxf`), which keep each entity's CAD layer in the `layer` field.

/// Whether a CAD layer name matches a pattern: ASCII case-insensitive, `*` any run of non-space characters.
pub fn matches(pattern: &str, name: &str) -> bool {
    let p: Vec<char> = pattern.trim().chars().map(|c| c.to_ascii_lowercase()).collect();
    let n: Vec<char> = name.chars().map(|c| c.to_ascii_lowercase()).collect();
    let (mut i, mut j) = (0, 0);
    // The last `*` seen and where in the name it started matching: backtracked to on a mismatch.
    let mut star: Option<(usize, usize)> = None;
    while let Some(&c) = n.get(j) {
        match p.get(i) {
            Some('*') => {
                star = Some((i, j));
                i += 1;
            }
            Some(&pc) if pc == c => {
                i += 1;
                j += 1;
            }
            _ => match star {
                Some((si, sj)) => {
                    // When backtracking through a *, don't skip spaces
                    if c == ' ' {
                        return false;
                    }
                    star = Some((si, sj + 1));
                    i = si + 1;
                    j = sj + 1;
                }
                None => return false,
            },
        }
    }
    p.get(i..).is_some_and(|rest| rest.iter().all(|c| *c == '*'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn patterns() {
        assert!(matches("ONG *", "ONG 300"));
        assert!(matches("ong *", "ONG 500"));
        assert!(matches("DAT_*", "DAT_CTSX_Congnghiep"));
        assert!(matches("*_Songsuoi", "QHDH_DAT_KHAC_Songsuoi"));
        assert!(matches("D*T*", "DAT_Anninh"));
        assert!(matches("*", ""));
        assert!(matches("Chialo", "chialo"));
        assert!(!matches("ONG *", "ong tnm B1000"));
        assert!(!matches("DAT_*", "QHDH_DAT_KHAC_Songsuoi"));
        assert!(!matches("Chialo", "Chialo2"));
        assert!(!matches("", "x"));
        assert!(matches(" ONG * ", "ONG 300"), "spaces around a pattern don't count");
    }
}
