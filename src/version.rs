
pub const R1_8: i32 = 47;
pub const R1_9: i32 = 107;
pub const R1_9_1: i32 = 108;
pub const R1_9_2: i32 = 109;
pub const R1_9_4: i32 = 110;
pub const R1_10: i32 = 210;
pub const R1_11: i32 = 315;
pub const R1_11_1: i32 = 316;
pub const R1_12: i32 = 335;
pub const R1_12_1: i32 = 338;
pub const R1_12_2: i32 = 340;
pub const R1_13: i32 = 393;
pub const R1_13_1: i32 = 401;
pub const R1_13_2: i32 = 404;
pub const R1_14: i32 = 477;
pub const R1_14_1: i32 = 480;
pub const R1_14_2: i32 = 485;
pub const R1_14_3: i32 = 490;
pub const R1_14_4: i32 = 498;
pub const R1_15: i32 = 573;
pub const R1_15_1: i32 = 575;
pub const R1_15_2: i32 = 578;
pub const R1_16: i32 = 735;
pub const R1_16_1: i32 = 736;
pub const R1_16_2: i32 = 751;
pub const R1_16_3: i32 = 753;
pub const R1_16_4: i32 = 754;
pub const R1_17: i32 = 755;
pub const R1_17_1: i32 = 756;
pub const R1_18: i32 = 757;
pub const R1_18_2: i32 = 758;
pub const R1_19: i32 = 759;
pub const R1_19_1: i32 = 760;
pub const R1_19_3: i32 = 761;
pub const R1_19_4: i32 = 762;
pub const R1_20: i32 = 763;
pub const R1_20_2: i32 = 764;
pub const R1_20_3: i32 = 765;
pub const R1_20_5: i32 = 766;
pub const R1_21: i32 = 767;
pub const R1_21_2: i32 = 768;
pub const R1_21_4: i32 = 769;

pub const SUPPORTED_VERSIONS: &[i32] = &[
    R1_20_2, R1_20_3, R1_20_5,
    R1_21, R1_21_2, R1_21_4,
];

pub fn is_supported(version: i32) -> bool {
    SUPPORTED_VERSIONS.contains(&version)
}
