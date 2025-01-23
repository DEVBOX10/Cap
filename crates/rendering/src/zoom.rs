use cap_project::{ZoomSegment, XY};

pub const ZOOM_DURATION: f64 = 1.0;

#[derive(Debug)]
pub struct SegmentsCursor<'a> {
    time: f64,
    segment: Option<&'a ZoomSegment>,
    prev_segment: Option<&'a ZoomSegment>,
    segments: &'a [ZoomSegment],
}

impl<'a> SegmentsCursor<'a> {
    pub fn new(time: f64, segments: &'a [ZoomSegment]) -> Self {
        match segments
            .iter()
            .position(|s| time > s.start && time <= s.end)
        {
            Some(segment_index) => SegmentsCursor {
                time,
                segment: Some(&segments[segment_index]),
                prev_segment: if segment_index > 0 {
                    Some(&segments[segment_index - 1])
                } else {
                    None
                },
                segments,
            },
            None => {
                let prev = segments
                    .iter()
                    .enumerate()
                    .rev()
                    .find(|(_, s)| s.end <= time);
                SegmentsCursor {
                    time,
                    segment: None,
                    prev_segment: prev.map(|(_, s)| s),
                    segments,
                }
            }
        }
    }
}

#[derive(Debug, PartialEq, Clone, Copy)]
pub struct SegmentBounds {
    pub top_left: XY<f64>,
    pub bottom_right: XY<f64>,
}

impl SegmentBounds {
    fn from_segment(segment: &ZoomSegment) -> Self {
        let position = match segment.mode {
            cap_project::ZoomMode::Auto => (0.0, 0.0),
            cap_project::ZoomMode::Manual { x, y } => (x, y),
        };

        let scaled_center = [
            position.0 as f64 * segment.amount,
            position.1 as f64 * segment.amount,
        ];
        let center_diff = [
            scaled_center[0] - position.0 as f64,
            scaled_center[1] - position.1 as f64,
        ];

        SegmentBounds::new(
            XY::new(0.0 - center_diff[0], 0.0 - center_diff[1]),
            XY::new(
                segment.amount - center_diff[0],
                segment.amount - center_diff[1],
            ),
        )
    }
}

impl SegmentBounds {
    pub fn new(top_left: XY<f64>, bottom_right: XY<f64>) -> Self {
        Self {
            top_left,
            bottom_right,
        }
    }

    pub fn default() -> Self {
        SegmentBounds::new(XY::new(0.0, 0.0), XY::new(1.0, 1.0))
    }
}

pub struct InterpolatedZoom {
    // the ratio of current zoom to the maximum amount for the current segment
    pub t: f64,
    pub bounds: SegmentBounds,
}

impl InterpolatedZoom {
    pub fn new(cursor: &SegmentsCursor) -> Self {
        let ease_in = bezier_easing::bezier_easing(0.1, 0.0, 0.3, 1.0).unwrap();
        let ease_out = bezier_easing::bezier_easing(0.5, 0.0, 0.5, 1.0).unwrap();

        let default = SegmentBounds::default();
        match (cursor.prev_segment, cursor.segment) {
            (Some(prev_segment), None) => {
                let zoom_t =
                    ease_out(t_clamp((cursor.time - prev_segment.end) / ZOOM_DURATION) as f32)
                        as f64;

                Self {
                    t: 1.0 - zoom_t,
                    bounds: {
                        let prev_segment_bounds = SegmentBounds::from_segment(prev_segment);

                        SegmentBounds::new(
                            prev_segment_bounds.top_left * (1.0 - zoom_t)
                                + default.top_left * zoom_t,
                            prev_segment_bounds.bottom_right * (1.0 - zoom_t)
                                + default.bottom_right * zoom_t,
                        )
                    },
                }
            }
            (None, Some(segment)) => {
                let t =
                    ease_in(t_clamp((cursor.time - segment.start) / ZOOM_DURATION) as f32) as f64;

                Self {
                    t,
                    bounds: {
                        let segment_bounds = SegmentBounds::from_segment(segment);

                        SegmentBounds::new(
                            default.top_left * (1.0 - t) + segment_bounds.top_left * t,
                            default.bottom_right * (1.0 - t) + segment_bounds.bottom_right * t,
                        )
                    },
                }
            }
            (Some(prev_segment), Some(segment)) => {
                let prev_segment_bounds = SegmentBounds::from_segment(prev_segment);
                let segment_bounds = SegmentBounds::from_segment(segment);

                let zoom_t =
                    ease_in(t_clamp((cursor.time - segment.start) / ZOOM_DURATION) as f32) as f64;

                // no gap
                if segment.start == prev_segment.end {
                    Self {
                        t: 1.0,
                        bounds: SegmentBounds::new(
                            prev_segment_bounds.top_left * (1.0 - zoom_t)
                                + segment_bounds.top_left * zoom_t,
                            prev_segment_bounds.bottom_right * (1.0 - zoom_t)
                                + segment_bounds.bottom_right * zoom_t,
                        ),
                    }
                }
                // small gap
                else if segment.start - prev_segment.end < ZOOM_DURATION {
                    // handling this is a bit funny, since we're not zooming in from 0 but rather
                    // from the previous value that the zoom out got interrupted at by the current segment

                    let min =
                        InterpolatedZoom::new(&SegmentsCursor::new(segment.start, cursor.segments));

                    Self {
                        t: (min.t * (1.0 - zoom_t)) + zoom_t,
                        bounds: {
                            let max = segment_bounds;

                            SegmentBounds::new(
                                min.bounds.top_left * (1.0 - zoom_t) + max.top_left * zoom_t,
                                min.bounds.bottom_right * (1.0 - zoom_t)
                                    + max.bottom_right * zoom_t,
                            )
                        },
                    }
                }
                // entirely separate
                else {
                    Self {
                        t: zoom_t,
                        bounds: SegmentBounds::new(
                            default.top_left * (1.0 - zoom_t) + segment_bounds.top_left * zoom_t,
                            default.bottom_right * (1.0 - zoom_t)
                                + segment_bounds.bottom_right * zoom_t,
                        ),
                    }
                }
            }
            _ => Self {
                t: 0.0,
                bounds: default,
            },
        }
    }
}

fn t_clamp(v: f64) -> f64 {
    v.clamp(0.0, 1.0)
}

#[cfg(test)]
mod segmentising {
    use cap_project::ZoomMode;

    use super::*;

    // Custom macro for floating-point near equality
    macro_rules! assert_f64_near {
        ($left:expr, $right:expr) => {
            let left = $left;
            let right = $right;
            assert!(
                (left - right).abs() < 1e-6,
                "assertion failed: `(left ~ right)` \n left: `{:?}`, \n right: `{:?}`",
                left,
                right
            )
        };
    }

    mod lerp_t {
        use super::*;

        fn lerp_test(cursor: SegmentsCursor, right: f64) {
            assert_f64_near!(interpolate_segment_t(&cursor), right);
        }

        #[test]
        fn one_segment() {
            let segments = vec![ZoomSegment {
                start: 2.0,
                end: 4.0,
                amount: 2.5,
                mode: ZoomMode::Manual { x: 0.0, y: 0.0 },
            }];

            lerp_test(SegmentsCursor::new(0.0, &segments), 0.0);
            lerp_test(SegmentsCursor::new(2.0, &segments), 0.0);
            lerp_test(
                SegmentsCursor::new(2.0 + ZOOM_DURATION * 0.1, &segments),
                0.1,
            );
            lerp_test(
                SegmentsCursor::new(2.0 + ZOOM_DURATION * 0.9, &segments),
                0.9,
            );
            lerp_test(SegmentsCursor::new(2.0 + ZOOM_DURATION, &segments), 1.0);
            lerp_test(SegmentsCursor::new(4.0, &segments), 1.0);
            lerp_test(
                SegmentsCursor::new(4.0 + ZOOM_DURATION * 0.2, &segments),
                0.8,
            );
            lerp_test(
                SegmentsCursor::new(4.0 + ZOOM_DURATION * 0.8, &segments),
                0.2,
            );
            lerp_test(SegmentsCursor::new(4.0 + ZOOM_DURATION, &segments), 0.0);
        }

        #[test]
        fn two_segments_no_gap() {
            let segments = vec![
                ZoomSegment {
                    start: 2.0,
                    end: 4.0,
                    amount: 2.5,
                    mode: ZoomMode::Manual { x: 0.0, y: 0.0 },
                },
                ZoomSegment {
                    start: 4.0,
                    end: 6.0,
                    amount: 4.0,
                    mode: ZoomMode::Manual { x: 0.0, y: 0.0 },
                },
            ];

            lerp_test(SegmentsCursor::new(4.0, &segments), 1.0);
            lerp_test(
                SegmentsCursor::new(4.0 + ZOOM_DURATION * 0.8, &segments),
                1.0,
            );
            lerp_test(
                SegmentsCursor::new(4.0 + ZOOM_DURATION * 0.8, &segments),
                1.0,
            );
            lerp_test(SegmentsCursor::new(4.0 + ZOOM_DURATION, &segments), 1.0);
            lerp_test(SegmentsCursor::new(6.0, &segments), 1.0);
            lerp_test(
                SegmentsCursor::new(6.0 + ZOOM_DURATION * 0.2, &segments),
                0.8,
            );
            lerp_test(
                SegmentsCursor::new(6.0 + ZOOM_DURATION * 0.8, &segments),
                0.2,
            );
            lerp_test(SegmentsCursor::new(7.0, &segments), 0.0);
        }

        #[test]
        fn two_segments_small_gap() {
            let segments = vec![
                ZoomSegment {
                    start: 2.0,
                    end: 4.0,
                    amount: 2.5,
                    mode: ZoomMode::Manual { x: 0.0, y: 0.0 },
                },
                ZoomSegment {
                    start: 4.0 + ZOOM_DURATION * 0.75,
                    end: 6.0,
                    amount: 2.5,
                    mode: ZoomMode::Manual { x: 0.0, y: 0.0 },
                },
            ];

            lerp_test(SegmentsCursor::new(4.0, &segments), 1.0);
            lerp_test(
                SegmentsCursor::new(4.0 + ZOOM_DURATION * 0.25, &segments),
                0.75,
            );
            lerp_test(
                SegmentsCursor::new(4.0 + ZOOM_DURATION * 0.5, &segments),
                0.5,
            );
            lerp_test(
                SegmentsCursor::new(4.0 + ZOOM_DURATION * 0.75, &segments),
                0.25,
            );
            lerp_test(
                SegmentsCursor::new(4.0 + ZOOM_DURATION * 0.75 + ZOOM_DURATION / 2.0, &segments),
                0.625,
            );
            lerp_test(
                SegmentsCursor::new(4.0 + ZOOM_DURATION * 0.75 + ZOOM_DURATION, &segments),
                1.0,
            );
            lerp_test(SegmentsCursor::new(6.0, &segments), 1.0);
            lerp_test(SegmentsCursor::new(6.0 + ZOOM_DURATION, &segments), 0.0);
        }

        #[test]
        fn two_segments_large_gap() {
            let segments = vec![
                ZoomSegment {
                    start: 2.0,
                    end: 4.0,
                    amount: 2.5,
                    mode: ZoomMode::Manual { x: 0.0, y: 0.0 },
                },
                ZoomSegment {
                    start: 7.0,
                    end: 9.0,
                    amount: 2.5,
                    mode: ZoomMode::Manual { x: 0.0, y: 0.0 },
                },
            ];

            lerp_test(SegmentsCursor::new(2.0, &segments), 0.0);
            lerp_test(SegmentsCursor::new(2.0 + ZOOM_DURATION, &segments), 1.0);
            lerp_test(SegmentsCursor::new(4.0, &segments), 1.0);
            lerp_test(SegmentsCursor::new(4.0 + ZOOM_DURATION, &segments), 0.0);
            lerp_test(SegmentsCursor::new(5.0, &segments), 0.0);
            lerp_test(SegmentsCursor::new(7.0, &segments), 0.0);
            lerp_test(SegmentsCursor::new(7.0 + ZOOM_DURATION, &segments), 1.0);
            lerp_test(SegmentsCursor::new(9.0, &segments), 1.0);
            lerp_test(SegmentsCursor::new(9.0 + ZOOM_DURATION, &segments), 0.0);
        }
    }

    mod lerp_bounds {
        use super::*;

        fn bounds_approx_eq(l: SegmentBounds, r: SegmentBounds) {
            assert_f64_near!(l.top_left.x, r.top_left.x);
            assert_f64_near!(l.top_left.y, r.top_left.y);
            assert_f64_near!(l.bottom_right.x, r.bottom_right.x);
            assert_f64_near!(l.bottom_right.y, r.bottom_right.y);
        }

        #[test]
        fn one_segment() {
            let segments = vec![ZoomSegment {
                start: 2.0,
                end: 4.0,
                amount: 2.5,
                mode: ZoomMode::Manual { x: 0.5, y: 0.5 },
            }];

            assert_eq!(
                interpolate_segment_bounds(&SegmentsCursor::new(2.0, &segments)),
                SegmentBounds::default()
            );
            assert_eq!(
                interpolate_segment_bounds(&SegmentsCursor::new(2.0 + ZOOM_DURATION, &segments)),
                SegmentBounds::new(XY::new(-0.75, -0.75), XY::new(1.75, 1.75))
            );
            assert_eq!(
                interpolate_segment_bounds(&SegmentsCursor::new(4.0, &segments)),
                SegmentBounds::new(XY::new(-0.75, -0.75), XY::new(1.75, 1.75))
            );
            assert_eq!(
                interpolate_segment_bounds(&SegmentsCursor::new(
                    4.0 + ZOOM_DURATION / 2.0,
                    &segments
                )),
                SegmentBounds::new(XY::new(-0.375, -0.375), XY::new(1.375, 1.375))
            );
        }

        #[test]
        fn two_segments_no_gap() {
            let segments = vec![
                ZoomSegment {
                    start: 2.0,
                    end: 4.0,
                    amount: 2.0,
                    mode: ZoomMode::Manual { x: 0.2, y: 0.2 },
                },
                ZoomSegment {
                    start: 4.0,
                    end: 6.0,
                    amount: 2.0,
                    mode: ZoomMode::Manual { x: 0.8, y: 0.8 },
                },
            ];

            bounds_approx_eq(
                interpolate_segment_bounds(&SegmentsCursor::new(4.0, &segments)),
                SegmentBounds::new(XY::new(-0.2, -0.2), XY::new(1.8, 1.8)),
            );
        }

        #[test]
        fn two_segments_with_gap() {
            let segments = vec![
                ZoomSegment {
                    start: 2.0,
                    end: 4.0,
                    amount: 2.0,
                    mode: ZoomMode::Manual { x: 0.0, y: 0.0 },
                },
                ZoomSegment {
                    start: 4.0 + ZOOM_DURATION * 0.5,
                    end: 6.0,
                    amount: 2.0,
                    mode: ZoomMode::Manual { x: 0.0, y: 0.0 },
                },
            ];

            bounds_approx_eq(
                interpolate_segment_bounds(&SegmentsCursor::new(4.0, &segments)),
                SegmentBounds::new(XY::new(0.0, 0.0), XY::new(2.0, 2.0)),
            );
            bounds_approx_eq(
                interpolate_segment_bounds(&SegmentsCursor::new(
                    4.0 + ZOOM_DURATION * 0.25,
                    &segments,
                )),
                SegmentBounds::new(XY::new(0.0, 0.0), XY::new(1.75, 1.75)),
            );
            bounds_approx_eq(
                interpolate_segment_bounds(&SegmentsCursor::new(
                    4.0 + ZOOM_DURATION * 0.5,
                    &segments,
                )),
                SegmentBounds::new(XY::new(0.0, 0.0), XY::new(1.5, 1.5)),
            );
            bounds_approx_eq(
                interpolate_segment_bounds(&SegmentsCursor::new(
                    4.0 + ZOOM_DURATION * 1.0,
                    &segments,
                )),
                SegmentBounds::new(XY::new(0.0, 0.0), XY::new(1.75, 1.75)),
            );
            bounds_approx_eq(
                interpolate_segment_bounds(&SegmentsCursor::new(
                    4.0 + ZOOM_DURATION * 1.5,
                    &segments,
                )),
                SegmentBounds::new(XY::new(0.0, 0.0), XY::new(2.0, 2.0)),
            );
        }
    }
}
