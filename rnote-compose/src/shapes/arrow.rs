use na::Rotation2;
use p2d::bounding_volume::Aabb;
use serde::{Deserialize, Serialize};

use crate::helpers::{AabbHelpers, Vector2Helpers};
use crate::shapes::ShapeBehaviour;
use crate::transform::TransformBehaviour;
use crate::Transform;

use super::{Line, Rectangle};

/// The following documentation assumes, the following graphic A:
/// ```
///          c
///         /|\
///        / | \
///       /  |  \
///      a   |   b
///          |
///          |
///          |
///          d
/// ```
/// Where:
///     - `a` represents the left helper line
///     - `b` represents the right helper line
///     - `c` represents the tip of the arrow
///     - `d` represents the main line
#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default)]
#[serde(default, rename = "arrow")]
pub struct Arrow {
    #[serde(rename = "start", with = "crate::serialize::na_vector2_f64_dp3")]
    /// The line start
    pub start: na::Vector2<f64>,
    #[serde(rename = "end", with = "crate::serialize::na_vector2_f64_dp3")]
    /// The line end
    pub tip: na::Vector2<f64>,
    tip_lines: TipLines,
}

impl TransformBehaviour for Arrow {
    fn translate(&mut self, offset: na::Vector2<f64>) {
        self.start += offset;
        self.tip += offset;
    }

    fn rotate(&mut self, angle: f64, center: na::Point2<f64>) {
        let mut isometry = na::Isometry2::identity();
        isometry.append_rotation_wrt_point_mut(&na::UnitComplex::new(angle), &center);

        self.start = (isometry * na::Point2::from(self.start)).coords;
        self.tip = (isometry * na::Point2::from(self.tip)).coords;
    }

    fn scale(&mut self, scale: na::Vector2<f64>) {
        self.start = self.start.component_mul(&scale);
        self.tip = self.tip.component_mul(&scale);
    }
}

impl ShapeBehaviour for Arrow {
    fn bounds(&self) -> Aabb {
        AabbHelpers::new_positive(na::Point2::from(self.start), na::Point2::from(self.tip))
    }

    fn hitboxes(&self) -> Vec<Aabb> {
        let n_splits = super::hitbox_elems_for_shape_len((self.tip - self.start).magnitude());

        self.split(n_splits)
            .into_iter()
            .map(|line| line.bounds())
            .collect()
    }
}

impl Arrow {
    /// Creating a new arrow with the given start and tip vectors.
    pub fn new(start: na::Vector2<f64>, tip: na::Vector2<f64>) -> Self {
        Self {
            start,
            tip,
            ..Self::default()
        }
    }

    /// creates a rect in the direction of the line, with a constant given width
    pub fn line_w_width_to_rect(&self, width: f64) -> Rectangle {
        let vec = self.tip - self.start;
        let magn = vec.magnitude();
        let angle = na::Rotation2::rotation_between(&na::Vector2::x(), &vec).angle();

        Rectangle {
            cuboid: p2d::shape::Cuboid::new(na::vector![magn * 0.5, width * 0.5]),
            transform: Transform::new_w_isometry(na::Isometry2::new(self.start + vec * 0.5, angle)),
        }
    }

    /// Splits itself given the no splits
    pub fn split(&self, n_splits: i32) -> Vec<Line> {
        (0..n_splits)
            .map(|i| {
                let sub_start = self
                    .start
                    .lerp(&self.tip, f64::from(i) / f64::from(n_splits));
                let sub_end = self
                    .start
                    .lerp(&self.tip, f64::from(i + 1) / f64::from(n_splits));

                Line {
                    start: sub_start,
                    end: sub_end,
                }
            })
            .collect::<Vec<Line>>()
    }

    /// to kurbo
    pub fn to_kurbo(&self) -> ArrowKurbo {
        let main = kurbo::Line::new(self.start.to_kurbo_point(), self.tip.to_kurbo_point());

        let rline = kurbo::Line::new(self.get_rline().to_kurbo_point(), self.tip.to_kurbo_point());

        let lline = kurbo::Line::new(self.get_lline().to_kurbo_point(), self.tip.to_kurbo_point());

        ArrowKurbo { main, rline, lline }
    }
}

/// This implementation holds the functions to get the vectors `a` and `b`.
impl Arrow {
    /// Returns the vector `a`.
    pub fn get_lline(&self) -> na::Vector2<f64> {
        let normed_dv = self.get_normed_dv();
        let rotation_matrix = self.get_rotation_matrix();
        rotation_matrix * normed_dv * self.tip_lines.length
    }

    /// Returns the vector `b`.
    pub fn get_rline(&self) -> na::Vector2<f64> {
        let normed_dv = self.get_normed_dv();
        let rotation_matrix = self.get_rotation_matrix().transpose();

        rotation_matrix * normed_dv * self.tip_lines.length
    }

    fn get_direction_vector(&self) -> na::Vector2<f64> {
        self.tip - self.start
    }

    fn get_normed_dv(&self) -> na::Vector2<f64> {
        let direction_vector = self.get_direction_vector();
        direction_vector / direction_vector.magnitude()
    }

    fn get_rotation_matrix(&self) -> Rotation2<f64> {
        Rotation2::new(self.tip_lines.angle)
    }
}

/// A helper struct to store the metadata of `a` and `b`.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(default, rename = "arrow_tip_lines")]
struct TipLines {
    pub angle: f64,
    pub length: f64,
}

impl TipLines {
    const DEFAULT_ANGLE: f64 = 45.0;
    const DEFAULT_LENGTH: f64 = 5.0;
}

impl Default for TipLines {
    fn default() -> Self {
        Self {
            angle: Self::DEFAULT_ANGLE,
            length: Self::DEFAULT_LENGTH,
        }
    }
}

/// A helper struct which contains the three lines for an arrow.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ArrowKurbo {
    pub main: kurbo::Line,
    pub rline: kurbo::Line,
    pub lline: kurbo::Line,
}
