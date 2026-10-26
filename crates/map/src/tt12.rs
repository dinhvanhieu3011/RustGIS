//! Land-use planning maps to the Ministry of Construction's Circular 12 (Thông tư 12/BXD): the
//! land-use groups, the codes each holds in the `layer` field, their colours and the pattern
//! images drawn at 1:[`IMAGE_SCALE`] or closer. Transcribed from the GeoServer style
//! `QH_SDD_TT12BXD.xml`; where it lists a code in two groups the later one wins, as it draws.

use crate::color::Rgba;
use crate::projection::normalize_vietnamese;
use crate::symbology::{Marker, OTHER_FILL, Renderer, Symbol};

/// The field holding a parcel's land-use code.
pub const FIELD: &str = "layer";
/// Parcels draw with their group's image at 1:`IMAGE_SCALE` or closer, in its colour further out.
pub const IMAGE_SCALE: f64 = 10_000.0;
/// Every parcel's outline at colour scales.
pub const OUTLINE: Rgba = Rgba::rgb(0x4D, 0x4D, 0x4D);

/// One land-use group.
#[derive(Debug, PartialEq)]
pub struct Group {
    /// The group's own code, also the name of its rule in the style.
    pub code: &'static str,
    pub name: &'static str,
    pub fill: Rgba,
    /// The outline at image scales.
    pub outline: Rgba,
    /// The pattern image's file name in the image folder.
    pub image: &'static str,
    /// The codes drawn as this group.
    pub codes: &'static [&'static str],
}

// DRAFT: the Vietnamese names await the user's review.
pub const GROUPS: [Group; 36] = [
    Group {
        code: "ODV",
        name: "Đất đơn vị ở",
        fill: Rgba::rgb(0xff, 0x7c, 0x01),
        outline: Rgba::rgb(0xff, 0x7c, 0x01),
        image: "DatDonViO.png",
        codes: &["ODV", "ODV01", "ODV02", "OM", "OM01", "OM02", "ODVM", "OBB", "OBG", "OBR", "OPD"],
    },
    Group {
        code: "ONN",
        name: "Đất ở nhóm nhà ở",
        fill: Rgba::rgb(0xec, 0xb0, 0x0a),
        outline: Rgba::rgb(0xec, 0xb0, 0x0a),
        image: "DatONhomNha.png",
        codes: &["ONN", "ONN01", "ONN02", "ONNM", "ONNUT", "ONNHT", "DOCNDA", "ONNXM", "ONNDD", "ONNHT01", "ONNHT02", "ONNPU", "ONNPO"],
    },
    Group {
        code: "OLK",
        name: "Đất ở liên kế",
        fill: Rgba::rgb(0xdf, 0x68, 0x00),
        outline: Rgba::rgb(0xdf, 0x68, 0x00),
        image: "DatOLienKe.png",
        codes: &["OLK01", "OLK03", "OLK"],
    },
    Group {
        code: "OCC",
        name: "Đất ở chung cư",
        fill: Rgba::rgb(0xff, 0xc2, 0x0f),
        outline: Rgba::rgb(0xff, 0xc2, 0x0f),
        image: "DatOChungCu.png",
        codes: &["OCC02", "OCC01", "OCC"],
    },
    Group {
        code: "OHH",
        name: "Đất ở hỗn hợp",
        fill: Rgba::rgb(0xdb, 0xaf, 0x2b),
        outline: Rgba::rgb(0xdb, 0xaf, 0x2b),
        image: "DatOHonHop.png",
        codes: &["OHH", "OHH01", "OLK02", "OHH02", "OHHDP", "HHXM", "ODT", "ODP"],
    },
    Group {
        code: "OBT",
        name: "Đất ở biệt thự",
        fill: Rgba::rgb(0xce, 0xb6, 0x26),
        outline: Rgba::rgb(0xce, 0xb6, 0x26),
        image: "DatOBietThu.png",
        codes: &["OBT", "OBT01", "OBT02", "OBT06", "OBT07", "MNC01", "OBT03", "OBT04", "OBT05", "OTT"],
    },
    Group {
        code: "OLX",
        name: "Đất ở làng xóm",
        fill: Rgba::rgb(0xd7, 0xd7, 0x6e),
        outline: Rgba::rgb(0xd7, 0xd7, 0x6e),
        image: "DatOLangXom.png",
        codes: &["OLX", "OLXHH", "OLX01", "DDD", "OLX02", "DONT", "OLXTDC", "OLX03", "OLX04", "OLX05", "OCT", "OCL", "OSP"],
    },
    Group {
        code: "CXGPC",
        name: "Đất công cộng đô thị",
        fill: Rgba::rgb(0xff, 0x00, 0x00),
        outline: Rgba::rgb(0xff, 0x00, 0x00),
        image: "DatCongCongDoThi.png",
        codes: &[
            "CXGPC", "CDT02", "CDT03", "CDT04", "CDT05", "CDT06", "CDT07", "CDT08", "DBH", "CDTHH", "CDTHH01", "CDTHH02", "CDTHH03",
            "CDTHH04", "CDTHH05", "CDTHH06", "CDTHH07", "TTCNNCKH", "CDTHH09", "CDTTP", "CDTKO", "CDTKO09", "CDTKO10", "CDTKO11",
            "CDTKO03", "CDTKO04", "CDTKO05", "CDTKO06", "CDTKO28", "CDTKO29", "CDTKO1", "CDTKO2", "CDTKO07", "CDTKO08", "CDTKO12",
            "CDTKO13", "CDTKO14", "CDTKO15", "CDTKO16", "CDTKO17", "CDTKO18", "CDTKO19", "CDTKO20", "CDTKO21", "CDTKO22", "CDTKO23",
            "CDTKO24", "CDTKO25", "CDTKO26", "CDTKO27", "CDTTK", "DXDCT", "CCNT", "CCSTT", "CCKC", "CCSBD", "SKT", "KVKT", "KVKKT",
            "CCGTHT", "CCXD", "CCCQ", "CCM", "CCM01", "CCM02", "CCM03", "CCM04", "ONNM01", "CCM05", "CCM06", "CCM07", "CCM08", "CCTTP",
            "CCTTPM", "CBD", "CBD03", "CBD01", "CBD02", "CTKS", "CCM09", "CTKS01", "CYT", "CYT01", "YTHH", "YTXM", "DCH", "DCH01", "DCH02",
            "DDVPT",
        ],
    },
    Group {
        code: "CDO",
        name: "Đất công cộng đơn vị ở",
        fill: Rgba::rgb(0xff, 0x53, 0x53),
        outline: Rgba::rgb(0xff, 0x53, 0x53),
        image: "DatCongCongDonViO.png",
        codes: &["CDO", "CDO01", "CDO02", "CDO04", "DCTQH", "CTKS02"],
    },
    Group {
        code: "TH",
        name: "Đất trường trung học phổ thông",
        fill: Rgba::rgb(0xa2, 0x35, 0x26),
        outline: Rgba::rgb(0xa2, 0x35, 0x26),
        image: "DatTruongTHPT.png",
        codes: &["TH", "TH01", "TH02", "TH03", "TH04", "TH05", "TH06", "THSC", "TH07", "THPT", "THPT01"],
    },
    Group {
        code: "THCS",
        name: "Đất trường trung học cơ sở, tiểu học, mầm non",
        fill: Rgba::rgb(0xb8, 0x60, 0x60),
        outline: Rgba::rgb(0xb8, 0x60, 0x60),
        image: "DatTruongTHCSTieuHocMamNon.png",
        codes: &[
            "THCS", "THCS01", "THTH", "THTH01", "THMN", "CDT01", "THMN01", "THMN02", "THDH", "THDH01", "THDT", "THHH", "THXDM", "THCN",
        ],
    },
    Group {
        code: "CXD",
        name: "Đất cây xanh đô thị",
        fill: Rgba::rgb(0xa2, 0xef, 0x25),
        outline: Rgba::rgb(0xa2, 0xef, 0x25),
        image: "DatCayXanhDT.png",
        codes: &[
            "CXD", "CXDTP", "CXD01", "CXD02", "CXD03", "CXD04", "CXD05", "CXD06", "CXD07", "CXD08", "CXD09", "CXD10", "CXD11", "CXD12",
            "CXDKO", "CXDNO", "CXCDTP", "CXTPHT", "CXTPM", "DVC", "DSVCX",
        ],
    },
    Group {
        code: "CXO",
        name: "Đất cây xanh đơn vị ở",
        fill: Rgba::rgb(0x33, 0xcc, 0x33),
        outline: Rgba::rgb(0x33, 0xcc, 0x33),
        image: "DatCayXanhDonViO.png",
        codes: &["CXO", "CXOTT", "CDO03", "CXCQ01", "CXCQ", "CXTN"],
    },
    Group {
        code: "DCN",
        name: "Đất công nghiệp",
        fill: Rgba::rgb(0x8e, 0x25, 0xcb),
        outline: Rgba::rgb(0x8e, 0x25, 0xcb),
        image: "DatCongNghiep.png",
        codes: &["DCN", "DCN01", "DCN02", "DCN-01", "DCN-02", "DCN-03", "DCNCC", "DCNCC01", "DCNKT", "DCNHH", "DCNXM", "DCN03", "DCN04"],
    },
    Group {
        code: "DKT",
        name: "Đất kho tàng",
        fill: Rgba::rgb(0x6a, 0x6a, 0xb1),
        outline: Rgba::rgb(0x6a, 0x6a, 0xb1),
        image: "DatKhoTang.png",
        codes: &["DKT", "DKB", "DKBHH", "DKBXM", "DT", "DPTQ", "DLGT"],
    },
    Group {
        code: "DGD",
        name: "Đất trung tâm đào tạo, nghiên cứu",
        fill: Rgba::rgb(0x1a, 0xb0, 0xe2),
        outline: Rgba::rgb(0x1a, 0xb0, 0xe2),
        image: "DatTrungTamDaoTao.png",
        codes: &["DGD"],
    },
    Group {
        code: "DCQ",
        name: "Đất cơ quan",
        fill: Rgba::rgb(0xce, 0x67, 0x00),
        outline: Rgba::rgb(0xce, 0x67, 0x00),
        image: "DatCoQuan.png",
        codes: &["DCQ", "DCQ01", "DCQ02", "DCQDT", "DCQND"],
    },
    Group {
        code: "DYT",
        name: "Đất trung tâm y tế",
        fill: Rgba::rgb(0xff, 0x66, 0xff),
        outline: Rgba::rgb(0xff, 0x66, 0xff),
        image: "DatTrungTamYTe.png",
        codes: &["DYT"],
    },
    Group {
        code: "DDL",
        name: "Đất du lịch",
        fill: Rgba::rgb(0xff, 0x33, 0xcc),
        outline: Rgba::rgb(0xff, 0x33, 0xcc),
        image: "DatDuLich.png",
        codes: &["DDL", "DDL01", "DDL02", "DDL03", "DDL04", "DDL06", "DDL05", "DDL07", "DDL08"],
    },
    Group {
        code: "DTG",
        name: "Đất tôn giáo",
        fill: Rgba::rgb(0xa1, 0x0c, 0x0c),
        outline: Rgba::rgb(0xa1, 0x0c, 0x0c),
        image: "DatTonGiao.png",
        codes: &["DTG", "DTG01", "DTGHH", "DTGXM"],
    },
    Group {
        code: "DHK",
        name: "Đất công trình hạ tầng kỹ thuật",
        fill: Rgba::rgb(0xa1, 0x7d, 0xc5),
        outline: Rgba::rgb(0xa1, 0x7d, 0xc5),
        image: "DatCongTrinhHTKT.png",
        codes: &[
            "DHK", "DHK02", "XDT", "DHK03", "DQT", "DQT1", "DHTKT", "DHTKT01", "DHTKT02", "DTDCV", "CXCQ02", "DHK01", "DHTKT03", "DHK07",
            "DHK08", "DHK09", "DHK10", "DHK11", "DHK04", "DHKGT", "BDX", "BDX02", "BDX03", "DSB", "DHK05", "DHK06", "BDXTT", "GDSN",
            "GDSNO", "GDS", "GDSDT", "DSB01", "DSB03", "DSB02",
        ],
    },
    Group {
        code: "XCD",
        name: "Đất cây xanh chuyên đề",
        fill: Rgba::rgb(0x2f, 0xd1, 0x2f),
        outline: Rgba::rgb(0x2f, 0xd1, 0x2f),
        image: "DatCayXanhChuyenDe.png",
        codes: &["XCD"],
    },
    Group {
        code: "DTT",
        name: "Đất trung tâm thể dục thể thao",
        fill: Rgba::rgb(0x00, 0xb8, 0x00),
        outline: Rgba::rgb(0x00, 0xb8, 0x00),
        image: "DatTrungTamTDTT.png",
        codes: &["DTT", "DTT01", "DTT02", "DTT03", "DTT04", "DTT05", "DTT06", "DTT07", "DTT08", "DTT09", "SGXM", "SGBT"],
    },
    Group {
        code: "CXCL",
        name: "Đất cây xanh cách ly",
        fill: Rgba::rgb(0x2a, 0xa6, 0x87),
        outline: Rgba::rgb(0x2a, 0xa6, 0x87),
        image: "DatCayXanhCachLy.png",
        codes: &["CXCL", "XCLPH", "CXKV"],
    },
    Group {
        code: "DAQ",
        name: "Đất an ninh, quốc phòng",
        fill: Rgba::rgb(0x66, 0x72, 0x3b),
        outline: Rgba::rgb(0x66, 0x72, 0x3b),
        image: "DatAnNinhQP.png",
        codes: &["DAQ", "DAQ01"],
    },
    Group {
        code: "DNT",
        name: "Đất nghĩa trang",
        fill: Rgba::rgb(0x77, 0x77, 0x77),
        outline: Rgba::rgb(0x77, 0x77, 0x77),
        image: "DatNghiaTrang.png",
        codes: &["DNT", "DNTTL"],
    },
    Group {
        code: "DNN",
        name: "Đất nông nghiệp",
        fill: Rgba::rgb(0x00, 0xff, 0x00),
        outline: Rgba::rgb(0x00, 0xff, 0x00),
        image: "DatNongNghiep.png",
        codes: &["DNN", "DNN01", "DNN02", "DNN03"],
    },
    Group {
        code: "DLN",
        name: "Đất lâm nghiệp",
        fill: Rgba::rgb(0x00, 0x88, 0x88),
        outline: Rgba::rgb(0x00, 0x88, 0x88),
        image: "DatLamNghiep.png",
        codes: &["DLN", "DHM"],
    },
    Group {
        code: "MNC",
        name: "Mặt nước",
        fill: Rgba::rgb(0x29, 0x86, 0xe3),
        outline: Rgba::rgb(0x29, 0x86, 0xe3),
        image: "MatNuoc.png",
        codes: &["MNC", "MNC06", "NTS", "BDX01", "MNC02", "MNC03", "MNC04", "MNC05", "MNTS", "DDP", "DDA", "DDT", "DDAVX", "DTR"],
    },
    Group {
        code: "VDT",
        name: "Vùng phát triển đô thị",
        fill: Rgba::rgb(0xfe, 0x74, 0x02),
        outline: Rgba::rgb(0xfe, 0x74, 0x02),
        image: "VungPhatTrienDothi.png",
        codes: &["VDT"],
    },
    Group {
        code: "VNT",
        name: "Vùng phát triển dân cư nông thôn",
        fill: Rgba::rgb(0xbf, 0xbf, 0x1c),
        outline: Rgba::rgb(0xbf, 0xbf, 0x1c),
        image: "VungPhatTrienDanCuNongThon.png",
        codes: &["VNT"],
    },
    Group {
        code: "VNN",
        name: "Vùng nông nghiệp",
        fill: Rgba::rgb(0x37, 0xff, 0x37),
        outline: Rgba::rgb(0x37, 0xff, 0x37),
        image: "VungNongNghiep.png",
        codes: &["VNN", "VNN-01"],
    },
    Group {
        code: "RDD",
        name: "Rừng đặc dụng",
        fill: Rgba::rgb(0x00, 0x78, 0x9c),
        outline: Rgba::rgb(0x00, 0x78, 0x9c),
        image: "RungDacDung.png",
        codes: &[
            "RDD", "DDN", "RNM", "RPH", "DKDC", "RT", "RPH01", "RSX", "DRT", "STR", "DCAQ", "RTN", "RTNTT", "DTCDL", "RCQ", "RCV", "RCN",
            "RCB",
        ],
    },
    Group {
        code: "VQG",
        name: "Vườn quốc gia",
        fill: Rgba::rgb(0x5c, 0x9f, 0x1a),
        outline: Rgba::rgb(0x5c, 0x9f, 0x1a),
        image: "VuonQuocGia.png",
        codes: &["VQG", "KBT", "KBV", "KAQ", "KHK", "KKT", "KCC", "KDL", "KCN", "KCN-DH", "KCN-NH", "KTT", "KGD"],
    },
    Group {
        code: "DNL",
        name: "Đất công trình năng lượng",
        fill: Rgba::rgb(0x45, 0x94, 0x9d),
        outline: Rgba::rgb(0xff, 0xbf, 0x7f),
        image: "DatNangLuong.png",
        codes: &["DNL"],
    },
    Group {
        code: "DLM",
        name: "Đất làm muối",
        fill: Rgba::rgb(0xbb, 0xd5, 0xe2),
        outline: Rgba::rgb(0xbb, 0xd5, 0xe2),
        image: "DatCayXanhChuyenDe.png",
        codes: &["DLM"],
    },
];

/// The group a code (as stored, exactly) draws as.
pub fn group_of(code: &str) -> Option<&'static Group> {
    GROUPS.iter().find(|g| g.codes.contains(&code))
}

/// The group a land-use name in words stands for (case, diacritics, spaces and punctuation
/// ignored), or its group code.
pub fn group_by_name(name: &str) -> Option<&'static Group> {
    let key = normalize_vietnamese(name);
    if key.is_empty() {
        return None;
    }
    GROUPS.iter().find(|g| normalize_vietnamese(g.name) == key || normalize_vietnamese(g.code) == key)
}

/// A Unique Values renderer on [`FIELD`] with a class per code, in its group's colour.
pub fn renderer() -> Renderer {
    let symbol = |fill: Rgba| Symbol { fill, stroke: OUTLINE, stroke_width: 0.7, size: 0.0, marker: Marker::Circle };
    let mut classes = Vec::new();
    let mut class_labels = std::collections::BTreeMap::new();
    for g in &GROUPS {
        for c in g.codes {
            classes.push((c.to_string(), symbol(g.fill)));
            class_labels.insert(c.to_string(), format!("{} ({c})", g.name));
        }
    }
    Renderer::Unique {
        field: FIELD.into(),
        classes,
        other: symbol(OTHER_FILL),
        ramp: Default::default(),
        class_labels,
        other_label: None,
        class_strokes: Default::default(),
        other_strokes: Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_matches_the_sld() {
        assert_eq!(GROUPS.len(), 36);
        assert_eq!(group_of("ODV01").map(|g| g.code), Some("ODV"));
        assert_eq!(group_of("CDO03").map(|g| g.code), Some("CXO"));
        assert_eq!(group_of("BDX01").map(|g| g.code), Some("MNC"));
        assert_eq!(group_of("ZZZ"), None);
        let g = |c: &str| GROUPS.iter().find(|g| g.code == c);
        assert_eq!(g("ODV").map(|g| g.fill), Some(Rgba::rgb(0xff, 0x7c, 0x01)));
        assert_eq!(g("DNL").map(|g| g.outline), Some(Rgba::rgb(0xff, 0xbf, 0x7f)));
        assert_eq!(g("DLM").map(|g| g.image), Some("DatCayXanhChuyenDe.png"));
        let distinct: std::collections::HashSet<_> = GROUPS.iter().flat_map(|g| g.codes).collect();
        assert_eq!(distinct.len(), 359);
        // No code in two groups.
        assert_eq!(GROUPS.iter().map(|g| g.codes.len()).sum::<usize>(), distinct.len());
        let names: std::collections::HashSet<_> = GROUPS.iter().map(|g| normalize_vietnamese(g.name)).collect();
        assert_eq!(names.len(), 36);
    }

    #[test]
    fn names_match_without_diacritics() {
        let want = Some("ODV");
        assert_eq!(group_by_name("Đất đơn vị ở").map(|g| g.code), want);
        assert_eq!(group_by_name("  dat don vi o ").map(|g| g.code), want);
        assert_eq!(group_by_name("odv").map(|g| g.code), want);
        assert_eq!(group_by_name("không có"), None);
        assert_eq!(group_by_name("  "), None);
    }

    #[test]
    fn renderer_colours_every_code() {
        let Renderer::Unique { field, classes, class_labels, .. } = renderer() else { panic!("not unique values") };
        assert_eq!(field, FIELD);
        assert_eq!(classes.len(), 359);
        assert!(classes.iter().any(|(k, s)| k == "OM01" && s.fill == Rgba::rgb(0xff, 0x7c, 0x01)));
        assert_eq!(class_labels.get("CDO03").map(String::as_str), Some("Đất cây xanh đơn vị ở (CDO03)"));
    }
}
