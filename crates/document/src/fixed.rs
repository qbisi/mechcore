//! Native Q32.32 arithmetic shared by document admission and simulation.

const Q32_ONE: i64 = 1 << 32;

/// `FPoint.RawSqrt`, the polynomial approximation of `FPCSMath.SqrtFastest`.
/// Both the input and output are Q32.32 values. A negative input returns
/// the native NaN sentinel; zero returns zero.
#[must_use]
pub fn fpcs_sqrt_fastest(value: i64) -> i64 {
    if value < 0 {
        return i64::MIN + 1;
    }
    if value == 0 {
        return 0;
    }
    let exponent = 31 - i32::try_from(value.leading_zeros()).unwrap_or(64);
    let normalized = if exponent >= 0 {
        value >> exponent
    } else {
        value.wrapping_shl(exponent.unsigned_abs())
    };
    let mut variable = (i64::from_ne_bytes(0xC000_0000_0000_0000_u64.to_ne_bytes())
        .wrapping_add(normalized.wrapping_shl(30)))
        >> 32;
    let coefficient = variable;
    variable = variable.wrapping_mul(0x0664_5730);
    variable =
        (i64::from_ne_bytes(0xF90F_54C4_0000_0000_u64.to_ne_bytes()).wrapping_add(variable)) >> 32;
    let coefficient = coefficient.wrapping_shl(2);
    variable = variable.wrapping_mul(coefficient);
    variable = (0x1FDA_0F0B_0000_0000_i64.wrapping_add(variable)) >> 32;
    variable = coefficient.wrapping_mul(variable);
    variable = (0x4000_0000_0000_0000_i64.wrapping_add(variable)) >> 32;
    let odd_factor = if exponent & 1 == 0 {
        Q32_ONE
    } else {
        0x0001_6A09_E664
    };
    let mut result = odd_factor.wrapping_mul(variable) >> 30;
    result &= !3;
    let half_exponent = exponent >> 1;
    if half_exponent >= 0 {
        result.wrapping_shl(half_exponent.cast_unsigned())
    } else {
        result >> half_exponent.unsigned_abs()
    }
}
