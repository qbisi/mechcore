use super::*;

pub(in crate::fight) const fn gcd(mut left: u64, mut right: u64) -> u64 {
    while right != 0 {
        let remainder = left % right;
        left = right;
        right = remainder;
    }
    left
}

pub(in crate::fight) fn native_time_units_to_steps(time_units: u64) -> u64 {
    let raw_time =
        i64::try_from((u128::from(time_units) << 32) / u128::from(TIME_UNITS_PER_SECOND))
            .unwrap_or(i64::MAX);
    q32_div(raw_time, NATIVE_LOGIC_DELTA_Q32)
        .max(0)
        .cast_unsigned()
        >> 32
}

#[cfg(test)]
pub(in crate::fight) fn rotation_distance(left: i64, right: i64) -> i64 {
    ((right.rem_euclid(360_000) - left.rem_euclid(360_000) + 540_000).rem_euclid(360_000) - 180_000)
        .abs()
}

pub(in crate::fight) fn mdeg_to_degrees_q32(value: i64) -> i64 {
    i64::try_from(i128::from(value) * i128::from(Q32_ONE) / 1_000).unwrap_or(if value < 0 {
        i64::MIN
    } else {
        i64::MAX
    })
}

pub(in crate::fight) fn degrees_q32_to_mdeg(value: i64) -> i64 {
    degrees_q32_to_unwrapped_mdeg(value) % 360_000
}

pub(in crate::fight) fn degrees_q32_to_unwrapped_mdeg(value: i64) -> i64 {
    let scaled = i128::from(value) * 1_000;
    let rounded = if scaled >= 0 {
        (scaled + i128::from(Q32_ONE / 2)) >> 32
    } else {
        -((-scaled + i128::from(Q32_ONE / 2)) >> 32)
    };
    i64::try_from(rounded).unwrap_or(if rounded < 0 { i64::MIN } else { i64::MAX })
}

pub(in crate::fight) fn rotate_towards_q32(current: i64, target: i64, maximum: i64) -> i64 {
    rotate_towards_q32_unwrapped(current, target, maximum).rem_euclid(360_i64 << 32)
}

pub(in crate::fight) fn rotate_towards_q32_unwrapped(
    current: i64,
    target: i64,
    maximum: i64,
) -> i64 {
    let full = 360_i64 << 32;
    let half = 180_i64 << 32;
    let current = current.rem_euclid(full);
    let target = target.rem_euclid(full);
    let mut delta = (target - current).rem_euclid(full);
    if super::rvo::fpoint_less_than(half, delta) {
        delta -= full;
    }
    current + delta.clamp(-maximum, maximum)
}

pub(in crate::fight) fn rotation_distance_q32(left: i64, right: i64) -> i64 {
    let full = 360_i64 << 32;
    let half = 180_i64 << 32;
    ((right.rem_euclid(full) - left.rem_euclid(full) + full + half).rem_euclid(full) - half).abs()
}

/// How far a point lies from a segment, in the fixed point the fight holds.
///
/// The build asks `LineRange.Overlaps(CircleRange)`, a rectangle from the
/// attacker to its target against a circle on the construction. This is the
/// same test written as a distance, which is what makes it one comparison
/// against a measured width.
pub(in crate::fight) fn distance_to_segment_q32(
    from: (i64, i64),
    to: (i64, i64),
    point: (i64, i64),
) -> i64 {
    let (dx, dz) = (
        i128::from(to.0) - i128::from(from.0),
        i128::from(to.1) - i128::from(from.1),
    );
    let (px, pz) = (
        i128::from(point.0) - i128::from(from.0),
        i128::from(point.1) - i128::from(from.1),
    );
    let length = dx * dx + dz * dz;
    if length == 0 {
        return magnitude(
            i64::try_from(px).unwrap_or(i64::MAX),
            i64::try_from(pz).unwrap_or(i64::MAX),
        );
    }
    let along = (px * dx + pz * dz).clamp(0, length);
    let (nearest_x, nearest_z) = (dx * along / length, dz * along / length);
    magnitude(
        i64::try_from(px - nearest_x).unwrap_or(i64::MAX),
        i64::try_from(pz - nearest_z).unwrap_or(i64::MAX),
    )
}

pub(in crate::fight) fn magnitude(x: i64, z: i64) -> i64 {
    integer_sqrt(i128::from(x) * i128::from(x) + i128::from(z) * i128::from(z))
}

#[cfg(test)]
pub(in crate::fight) fn q32_distance_within(
    source_x_q32: i64,
    source_z_q32: i64,
    target_x_q32: i64,
    target_z_q32: i64,
    bound_space: i64,
) -> bool {
    let dx = i128::from(target_x_q32) - i128::from(source_x_q32);
    let dz = i128::from(target_z_q32) - i128::from(source_z_q32);
    let bound = i128::from(space_to_q32(bound_space).max(0));
    dx.saturating_mul(dx).saturating_add(dz.saturating_mul(dz)) <= bound.saturating_mul(bound)
}

pub(in crate::fight) fn space_to_q32(value: i64) -> i64 {
    i64::try_from(i128::from(value) * i128::from(Q32_ONE) / i128::from(SPACE_UNITS_PER_METER))
        .unwrap_or(if value < 0 { i64::MIN } else { i64::MAX })
}

pub(in crate::fight) fn q32_to_space_rounded(value: i64) -> i64 {
    let scaled = i128::from(value) * i128::from(SPACE_UNITS_PER_METER);
    let rounded = if scaled >= 0 {
        (scaled + i128::from(Q32_ONE / 2)) >> 32
    } else {
        -((-scaled + i128::from(Q32_ONE / 2)) >> 32)
    };
    i64::try_from(rounded).unwrap_or(if rounded < 0 { i64::MIN } else { i64::MAX })
}

pub(in crate::fight) fn q32_mul(left: i64, right: i64) -> i64 {
    i64::try_from((i128::from(left) * i128::from(right)) >> 32).unwrap_or({
        if (left < 0) == (right < 0) {
            i64::MAX
        } else {
            i64::MIN
        }
    })
}

pub(in crate::fight) fn q32_div(numerator: i64, denominator: i64) -> i64 {
    if denominator == 0 {
        return if numerator < 0 { i64::MIN } else { i64::MAX };
    }
    let scaled = u128::from(numerator.unsigned_abs()) << 32;
    let divisor = u128::from(denominator.unsigned_abs());
    let quotient = scaled / divisor;
    let remainder = scaled % divisor;
    let rounded = quotient.saturating_add(u128::from(remainder.saturating_mul(2) >= divisor));
    if (numerator < 0) == (denominator < 0) {
        i64::try_from(rounded).unwrap_or(i64::MAX)
    } else {
        i64::try_from(rounded)
            .ok()
            .and_then(i64::checked_neg)
            .unwrap_or(i64::MIN)
    }
}

pub(in crate::fight) fn native_q32_magnitude(x: i64, z: i64) -> i64 {
    fpcs_sqrt_fastest(q32_mul(x, x).saturating_add(q32_mul(z, z)))
}

pub(in crate::fight) fn native_q32_magnitude_3d(x: i64, y: i64, z: i64) -> i64 {
    fpcs_sqrt_fastest(
        q32_mul(x, x)
            .saturating_add(q32_mul(y, y))
            .saturating_add(q32_mul(z, z)),
    )
}

pub(crate) fn fpcs_sqrt_fastest(value: i64) -> i64 {
    if value <= 0 {
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

pub(in crate::fight) fn q32_exponent(value: i64) -> i32 {
    debug_assert!(value > 0);
    31 - i32::try_from(value.leading_zeros()).unwrap_or(64)
}

pub(in crate::fight) fn normalize_q32(value: i64, exponent: i32) -> i64 {
    if exponent >= 0 {
        value >> exponent
    } else {
        value.wrapping_shl(exponent.unsigned_abs())
    }
}

pub(in crate::fight) fn fpcs_atan2_div_fastest(y: i64, x: i64) -> i32 {
    debug_assert!(y >= 0 && x > 0 && y <= x);
    let exponent = q32_exponent(x);
    let normalized_y = normalize_q32(y, exponent);
    let normalized_x = normalize_q32(x, exponent);
    let mut variable = (i64::from_ne_bytes(0xC000_0000_0000_0000_u64.to_ne_bytes())
        .wrapping_add(normalized_x.wrapping_shl(30)))
        >> 32;
    variable = variable.wrapping_mul(0x279B_5BB0);
    let coefficient = ((i64::from_ne_bytes(0xDD58_0FC7_0000_0000_u64.to_ne_bytes())
        .wrapping_add(variable))
        >> 32)
        .wrapping_mul(
            ((i64::from_ne_bytes(0xC000_0000_0000_0000_u64.to_ne_bytes())
                .wrapping_add(normalized_x.wrapping_shl(30)))
                >> 32)
                .wrapping_shl(2),
        );
    let polynomial = (0x37FD_4590_0000_0000_i64.wrapping_add(coefficient)) >> 32;
    let argument = ((i64::from_ne_bytes(0xC000_0000_0000_0000_u64.to_ne_bytes())
        .wrapping_add(normalized_x.wrapping_shl(30)))
        >> 32)
        .wrapping_shl(2);
    let polynomial = polynomial.wrapping_mul(argument);
    let polynomial = (i64::from_ne_bytes(0xC0C3_D3BF_0000_0000_u64.to_ne_bytes())
        .wrapping_add(polynomial))
        >> 32;
    let polynomial = polynomial.wrapping_mul(argument);
    let polynomial = (0x4000_0000_0000_0000_i64.wrapping_add(polynomial)) >> 32;
    let y_quarter = normalized_y >> 2;
    i32::try_from(polynomial.wrapping_mul(y_quarter) >> 30).unwrap_or(i32::MAX)
}

pub(in crate::fight) fn fpcs_atan_polynomial(divided: i32, sign_mask: i64) -> i64 {
    let variable = i64::from(divided);
    let mut polynomial = variable.wrapping_mul(0x2651_FC38);
    polynomial = (i64::from_ne_bytes(0xE8C5_3128_0000_0000_u64.to_ne_bytes())
        .wrapping_add(polynomial))
        >> 32;
    let argument = variable.wrapping_shl(2);
    polynomial = polynomial.wrapping_mul(argument);
    polynomial = (i64::from_ne_bytes(0xFFE4_A871_0000_0000_u64.to_ne_bytes())
        .wrapping_add(polynomial))
        >> 32;
    polynomial = polynomial.wrapping_mul(argument);
    polynomial = (0x4005_9E04_0000_0000_i64.wrapping_add(polynomial)) >> 32;
    polynomial = polynomial.wrapping_mul(argument) >> 30;
    (polynomial & !3) ^ sign_mask
}

pub(crate) fn fpcs_atan2_fastest(y: i64, x: i64) -> i64 {
    const PI_OVER_TWO: i64 = 0x1921_FB544;
    const PI: i64 = 0x3243_F6A89;
    if x == 0 {
        return match y.cmp(&0) {
            Ordering::Greater => PI_OVER_TWO,
            Ordering::Less => -PI_OVER_TWO,
            Ordering::Equal => 0,
        };
    }
    let absolute_x = x.saturating_abs();
    let absolute_y = y.saturating_abs();
    let sign_mask = (x ^ y) >> 63;
    if absolute_x < absolute_y {
        let divided = fpcs_atan2_div_fastest(absolute_x, absolute_y);
        let approximate = fpcs_atan_polynomial(divided, sign_mask);
        if y > 0 {
            PI_OVER_TWO.wrapping_sub(approximate)
        } else {
            (-PI_OVER_TWO).wrapping_sub(approximate)
        }
    } else {
        let divided = fpcs_atan2_div_fastest(absolute_y, absolute_x);
        let approximate = fpcs_atan_polynomial(divided, sign_mask);
        if x > 0 {
            approximate
        } else if y >= 0 {
            approximate.wrapping_add(PI)
        } else {
            approximate.wrapping_sub(PI)
        }
    }
}

pub(crate) fn fpcs_acos_fastest(value: i64) -> i64 {
    let complement = q32_mul(Q32_ONE.saturating_sub(value), Q32_ONE.saturating_add(value));
    fpcs_atan2_fastest(fpcs_sqrt_fastest(complement), value)
}

#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_possible_wrap,
    reason = "the 32-bit truncation and sign reinterpretation reproduce the build's Q32.32 arithmetic"
)]
pub(crate) fn fpcs_sin_fastest(value: i64) -> i64 {
    let turn = q32_mul(value, 0x28BE_60DC) as i32;
    let doubled = turn.wrapping_mul(2);
    let folded = 0x8000_0000_u32.wrapping_sub(turn as u32) as i32;
    let coordinate = i64::from(if doubled ^ turn >= 0 { turn } else { folded });
    let scaled_coordinate = coordinate.wrapping_mul(4);
    let squared = coordinate.wrapping_mul(scaled_coordinate) >> 32;
    let coefficient = i64::from_ne_bytes(0xD6CF_6F97_0000_0000_u64.to_ne_bytes())
        .wrapping_add(squared.wrapping_mul(0x12A2_8C60))
        >> 32;
    let polynomial = i64::from_ne_bytes(0x6487_ED51_0000_0000_u64.to_ne_bytes())
        .wrapping_add(coefficient.wrapping_mul(squared).wrapping_mul(4))
        >> 32;
    polynomial.wrapping_mul(scaled_coordinate) >> 30 & !3
}

pub(crate) fn fpcs_cos_fastest(value: i64) -> i64 {
    fpcs_sin_fastest(value.saturating_add(0x1_921F_B544))
}

pub(in crate::fight) fn integer_sqrt(value: i128) -> i64 {
    if value <= 0 {
        return 0;
    }
    let value = u128::try_from(value).unwrap_or(u128::MAX);
    let mut low = 0_u128;
    let mut high = value.min(u128::from(u64::MAX));
    while low < high {
        let middle = (low + high).div_ceil(2);
        if middle <= value / middle {
            low = middle;
        } else {
            high = middle - 1;
        }
    }
    i64::try_from(low).unwrap_or(i64::MAX)
}

#[cfg(test)]
pub(in crate::fight) fn direction_mdeg(dx: i64, dz: i64) -> i64 {
    direction_mdeg_q32_raw(space_to_q32(dx), space_to_q32(dz))
}

#[cfg(test)]
pub(in crate::fight) fn direction_mdeg_q32_raw(dx: i64, dz: i64) -> i64 {
    degrees_q32_to_mdeg(direction_degrees_q32_raw(dx, dz))
}

pub(in crate::fight) fn direction_degrees_q32_raw(dx: i64, dz: i64) -> i64 {
    if dx == 0 && dz == 0 {
        return 0;
    }
    let magnitude = native_q32_magnitude(dx, dz);
    if magnitude <= 0 {
        return 0;
    }
    let cosine = q32_div(dz, magnitude).clamp(-Q32_ONE, Q32_ONE);
    let radians = fpcs_acos_fastest(cosine);
    let degrees = q32_mul(radians, 0x0039_4BB8_34C8);
    let degrees = if dx < 0 {
        (360_i64 << 32).saturating_sub(degrees)
    } else {
        degrees
    };
    degrees.rem_euclid(360_i64 << 32)
}
