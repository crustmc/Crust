use const_format::formatcp;
use lazy_static::lazy_static;
use paste::paste;

use std::collections::HashMap;

// pub const R1_8: i32 = 47;
// pub const R1_9: i32 = 107;
// pub const R1_9_1: i32 = 108;
// pub const R1_9_2: i32 = 109;
// pub const R1_9_4: i32 = 110;
// pub const R1_10: i32 = 210;
// pub const R1_11: i32 = 315;
// pub const R1_11_1: i32 = 316;
// pub const R1_12: i32 = 335;
// pub const R1_12_1: i32 = 338;
// pub const R1_12_2: i32 = 340;
// pub const R1_13: i32 = 393;
// pub const R1_13_1: i32 = 401;
// pub const R1_13_2: i32 = 404;
// pub const R1_14: i32 = 477;
// pub const R1_14_1: i32 = 480;
// pub const R1_14_2: i32 = 485;
// pub const R1_14_3: i32 = 490;
// pub const R1_14_4: i32 = 498;
// pub const R1_15: i32 = 573;
// pub const R1_15_1: i32 = 575;
// pub const R1_15_2: i32 = 578;
// pub const R1_16: i32 = 735;
// pub const R1_16_1: i32 = 736;
// pub const R1_16_2: i32 = 751;
// pub const R1_16_3: i32 = 753;
// pub const R1_16_4: i32 = 754;
// pub const R1_17: i32 = 755;
// pub const R1_17_1: i32 = 756;
// pub const R1_18: i32 = 757;
// pub const R1_18_2: i32 = 758;
// pub const R1_19: i32 = 759;
// pub const R1_19_1: i32 = 760;
// pub const R1_19_3: i32 = 761;
// pub const R1_19_4: i32 = 762;
// pub const R1_20: i32 = 763;
// pub const R1_20_2: i32 = 764;
// pub const R1_20_3: i32 = 765;
// pub const R1_20_5: i32 = 766;
// pub const R1_21: i32 = 767;
// pub const R1_21_2: i32 = 768;
// pub const R1_21_4: i32 = 769;

macro_rules! versions {
    ($(($name:ident, $name_str:literal, $id:literal))*) => {
        $(
            pub const $name: i32 = $id;
            paste! {
                pub const [<$name _NAME>]: &str = $name_str;
            }
        )*
        
        pub const ALL_VERSIONS: &[i32] = &[$($name),*];
        paste! {
            pub const ALL_VERSION_NAMES: &[&str] = &[$([<$name _NAME>]),*];
            lazy_static! {
                pub static ref ALL_VERSION_INDICES: HashMap<i32, usize> = {
                    let mut map = HashMap::new();
                    $(
                        map.insert($name, ALL_VERSIONS.iter().position(|&v| v == $name).unwrap());
                    )*
                    map
                };
            }
        }
    };
}

versions! {
    (R1_8, "1.8.x", 47)
    (R1_9, "1.9", 107)
    (R1_9_1, "1.9.1", 108)
    (R1_9_2, "1.9.2", 109)
    (R1_9_4, "1.9.4", 110)
    (R1_10, "1.10", 210)
    (R1_11, "1.11", 315)
    (R1_11_1, "1.11.1", 316)
    (R1_12, "1.12", 335)
    (R1_12_1, "1.12.1", 338)
    (R1_12_2, "1.12.2", 340)
    (R1_13, "1.13", 393)
    (R1_13_1, "1.13.1", 401)
    (R1_13_2, "1.13.2", 404)
    (R1_14, "1.14", 477)
    (R1_14_1, "1.14.1", 480)
    (R1_14_2, "1.14.2", 485)
    (R1_14_3, "1.14.3", 490)
    (R1_14_4, "1.14.4", 498)
    (R1_15, "1.15", 573)
    (R1_15_1, "1.15.1", 575)
    (R1_15_2, "1.15.2", 578)
    (R1_16, "1.16", 735)
    (R1_16_1, "1.16.1", 736)
    (R1_16_2, "1.16.2", 751)
    (R1_16_3, "1.16.3", 753)
    (R1_16_4, "1.16.4", 754)
    (R1_17, "1.17", 755)
    (R1_17_1, "1.17.1", 756)
    (R1_18, "1.18", 757)
    (R1_18_2, "1.18.2", 758)
    (R1_19, "1.19", 759)
    (R1_19_1, "1.19.1", 760)
    (R1_19_3, "1.19.3", 761)
    (R1_19_4, "1.19.4", 762)
    (R1_20, "1.20", 763)
    (R1_20_2, "1.20.2", 764)
    (R1_20_3, "1.20.3", 765)
    (R1_20_5, "1.20.5", 766)
    (R1_21, "1.21", 767)
    (R1_21_2, "1.21.2", 768)
    (R1_21_4, "1.21.4", 769)
}

pub const SUPPORTED_VERSIONS: &[i32] = &[
    R1_20_2, R1_20_3, R1_20_5,
    R1_21, R1_21_2, R1_21_4,
];

pub const SUPPORTED_VERSION_RANGE: &str = formatcp!("{}-{}", ALL_VERSION_NAMES[0], ALL_VERSION_NAMES[ALL_VERSION_NAMES.len() - 1]);

pub fn is_supported(version: i32) -> bool {
    SUPPORTED_VERSIONS.contains(&version)
}
