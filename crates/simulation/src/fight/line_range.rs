//! `LineRange`'s rectangle overlap, used by a sweep's target-tree query.

use super::{Q32_ONE, path_finding, q32_div, rvo, search::TargetActorRect};
use rvo::FixedVec2;

pub(in crate::fight) struct LineRange {
    start: FixedVec2,
    end: FixedVec2,
    width: i64,
    vertices: [FixedVec2; 4],
}

impl LineRange {
    /// `LineRange.ctor`: its four vertices, from the normalized perpendicular.
    pub(in crate::fight) fn new(start: FixedVec2, end: FixedVec2, width: i64) -> Self {
        let direction = end.sub(start);
        let half = FixedVec2 {
            x: -direction.y,
            y: direction.x,
        }
        .normalized()
        .mul(width)
        .div(2 * Q32_ONE);
        Self {
            start,
            end,
            width,
            vertices: [
                start.add(half),
                start.sub(half),
                end.add(half),
                end.sub(half),
            ],
        }
    }

    /// `LineRange.Overlaps(RectRange)`: contained corners, contained strip
    /// vertices, then the two long edges crossing the rectangle.
    pub(in crate::fight) fn overlaps_rect(&self, rect: TargetActorRect) -> bool {
        let corners = [
            FixedVec2 {
                x: rect.max_x,
                y: rect.max_z,
            },
            FixedVec2 {
                x: rect.max_x,
                y: rect.min_z,
            },
            FixedVec2 {
                x: rect.min_x,
                y: rect.max_z,
            },
            FixedVec2 {
                x: rect.min_x,
                y: rect.min_z,
            },
        ];
        corners.iter().any(|point| {
            path_finding::line_range_contains(
                (self.start.x, self.start.y),
                (self.end.x, self.end.y),
                self.width,
                (point.x, point.y),
            )
        }) || self.vertices.iter().any(|point| {
            rect.contains(TargetActorRect {
                min_x: point.x,
                min_z: point.y,
                max_x: point.x,
                max_z: point.y,
            })
        }) || is_line_pass_through_rect(&corners, self.vertices[0], self.vertices[2])
            || is_line_pass_through_rect(&corners, self.vertices[1], self.vertices[3])
    }
}

/// `FightUtility.IsLinePassThroughRect`: its slope, intercept and vertical
/// coordinate are converted to integers before the corner-side tests.
#[allow(
    clippy::cast_possible_truncation,
    reason = "native FPoint-to-int conversion keeps the signed upper 32 bits"
)]
fn is_line_pass_through_rect(corners: &[FixedVec2; 4], start: FixedVec2, end: FixedVec2) -> bool {
    let dx = end.x.wrapping_sub(start.x);
    let sides = if rvo::fpoint_equal(dx, 0) {
        let line_x = i64::from((start.x >> 32) as i32) << 32;
        corners.map(|point| rvo::fpoint_greater_or_equal(point.x, line_x))
    } else {
        let slope = i64::from((q32_div(end.y.wrapping_sub(start.y), dx) >> 32) as i32);
        let intercept =
            i64::from((start.y.wrapping_sub(start.x.wrapping_mul(slope)) >> 32) as i32) << 32;
        corners.map(|point| {
            rvo::fpoint_greater_than(
                point
                    .y
                    .wrapping_sub(point.x.wrapping_mul(slope))
                    .wrapping_sub(intercept),
                0,
            )
        })
    };
    sides.iter().any(|side| *side != sides[0])
}
