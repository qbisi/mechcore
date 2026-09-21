use super::*;

#[test]
fn native_delta_distinguishes_1799_from_1800_time_units() {
    assert_eq!(native_time_units_to_steps(1_799), 17);
    assert_eq!(native_time_units_to_steps(1_800), 18);
}

#[test]
fn rotation_distance_uses_the_shortest_wrapped_arc() {
    assert_eq!(rotation_distance(359_000, 1_000), 2_000);
    assert_eq!(rotation_distance(1_000, 359_000), 2_000);
    assert_eq!(rotation_distance(20_000, 60_000), 40_000);
}

#[test]
fn exact_half_turn_uses_the_native_positive_direction() {
    let maximum = 6_i64 << 32;
    assert_eq!(
        rotate_towards_q32_unwrapped(0, 180_i64 << 32, maximum),
        maximum
    );
    assert_eq!(
        rotate_towards_q32_unwrapped(0, (180_i64 << 32) + 43, maximum),
        maximum
    );
    assert_eq!(
        rotate_towards_q32_unwrapped(0, (180_i64 << 32) + 44, maximum),
        -maximum
    );
    assert_eq!(
        rotate_towards_q32_unwrapped(180_i64 << 32, 0, maximum),
        186_i64 << 32
    );
}

#[test]
fn positive_body_rotation_exposes_the_exact_full_turn_for_one_tick() {
    let config = SimulationConfig::load().unwrap();
    let mut actor = Actor::new(
        test_placement(0, 0, 0, 0),
        config.units.get("crawler").unwrap().clone(),
        7,
    );
    actor.set_body_rotation(mdeg_to_degrees_q32(354_000));

    actor.rotate_body_towards(mdeg_to_degrees_q32(30_000));
    assert_eq!(degrees_q32_to_mdeg(actor.body_rotation_q32), 0);
    assert_eq!(actor.body_rotation, 360_000);

    actor.rotate_body_towards(mdeg_to_degrees_q32(30_000));
    assert_eq!(actor.body_rotation, 6_000);
}

#[test]
fn native_hundredth_constant_is_not_rationally_rounded() {
    assert_eq!(C0_01_RAW, 42_949_672);
    assert_eq!(q32_mul(30_i64 << 32, C0_01_RAW), 1_288_490_160);
    assert_ne!(C0_01_RAW, q32_div(Q32_ONE, 100_i64 << 32));
}

#[test]
fn native_fastest_angle_quantizes_small_jitter_to_forward() {
    assert_eq!(direction_mdeg(-600, 99_200), 0);
    assert_eq!(direction_mdeg(600, -99_200), 180_000);
    assert_eq!(direction_mdeg(1_000, 151_400), 853);
}

#[test]
fn raw_q32_velocity_preserves_arclight_target_angle_precision() {
    assert_eq!(
        direction_mdeg_q32_raw(-198_556_428, -30_061_443_202),
        180_812
    );
    assert_eq!(direction_mdeg(-46, -6_999), 180_811);
}

#[test]
fn q32_normalized_velocity_matches_frozen_arclight_delta() {
    let speed = space_to_q32(7_000);
    let reconstructed =
        normalized_velocity_q32_raw(space_to_q32(-1_000), space_to_q32(-151_400), speed);
    assert_eq!(reconstructed, (-198_556_428, -30_061_443_202));
    assert_eq!(
        (
            q32_to_space_rounded(reconstructed.0),
            q32_to_space_rounded(reconstructed.1),
        ),
        (-46, -6_999)
    );

    let c0_1 = 0x1999_9999;
    let native_raw = normalized_velocity_q32_raw(-10 * c0_1, -150 * Q32_ONE - 14 * c0_1, speed);
    assert_eq!(native_raw, reconstructed);
}

#[test]
fn q32_clamp_magnitude_stops_at_a_near_target_point() {
    let dx = space_to_q32(100);
    let dz = space_to_q32(-50);
    let speed = space_to_q32(7_123);
    let maximum = q32_mul(speed, NATIVE_LOGIC_DELTA_Q32);

    assert!(native_q32_magnitude(dx, dz) < maximum);
    assert_eq!(clamp_magnitude_q32_raw(dx, dz, maximum), (dx, dz));

    let far_dx = space_to_q32(1_000);
    let far_dz = space_to_q32(-10_000);
    let magnitude = native_q32_magnitude(far_dx, far_dz);
    let reciprocal = q32_div(Q32_ONE, magnitude);
    let native_order = (
        q32_mul(q32_mul(far_dx, reciprocal), maximum),
        q32_mul(q32_mul(far_dz, reciprocal), maximum),
    );
    let old_grouping = (
        q32_mul(
            q32_mul(q32_mul(far_dx, reciprocal), speed),
            NATIVE_LOGIC_DELTA_Q32,
        ),
        q32_mul(
            q32_mul(q32_mul(far_dz, reciprocal), speed),
            NATIVE_LOGIC_DELTA_Q32,
        ),
    );
    assert_ne!(native_order, old_grouping);
    assert_eq!(
        clamp_magnitude_q32_raw(far_dx, far_dz, maximum),
        native_order
    );
}

#[test]
fn q32_clamp_magnitude_preserves_a_tolerance_equal_zero_speed_delta() {
    // FPoint's comparison treats this squared magnitude (28 raw) as
    // equal to zero, so FVector2.ClampMagnitude returns the input delta.
    assert_eq!(
        clamp_magnitude_q32_raw(43_007, -347_649, 0),
        (43_007, -347_649)
    );
    assert_eq!(clamp_magnitude_q32_raw(0, Q32_ONE, 0), (0, 0));
}
