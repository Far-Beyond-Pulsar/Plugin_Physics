//! Painting for the material previews: a bounce plot and an incline diagram.
//!
//! Both are drawn with GPUI paths inside a `canvas`, from the closed-form
//! results in `assets::material`, so they respond instantly as sliders move.
//! Axis labels are ordinary GPUI text laid over the canvas by the caller.

use gpui::*;
use ui::PixelsExt;

use crate::assets::material::{BounceArc, SlopeState};
use crate::editors::common::paint::{arrow, dashed_line, dot, fill_polygon, pt, stroke_polyline, ChartColors};

/// Space left around the bounce plot for the axes, in pixels.
pub const PLOT_LEFT: f32 = 40.0;
pub const PLOT_RIGHT: f32 = 14.0;
pub const PLOT_TOP: f32 = 12.0;
pub const PLOT_BOTTOM: f32 = 24.0;

// ── Bounce plot ────────────────────────────────────────────────────────────

/// Everything the bounce plot draws.
#[derive(Clone)]
pub struct BouncePlot {
    pub arcs: Vec<BounceArc>,
    pub gravity: f32,
    pub drop_height: f32,
    /// The time span shown (at least the path's own length).
    pub duration: f32,
    pub colors: ChartColors,
}

impl BouncePlot {
    pub fn paint(&self, bounds: Bounds<Pixels>, window: &mut Window) {
        let c = &self.colors;
        let (ox, oy) = (bounds.origin.x.as_f32(), bounds.origin.y.as_f32());
        let (w, h) = (bounds.size.width.as_f32(), bounds.size.height.as_f32());
        let (left, right) = (ox + PLOT_LEFT, ox + w - PLOT_RIGHT);
        let (top, bottom) = (oy + PLOT_TOP, oy + h - PLOT_BOTTOM);
        if right - left < 10.0 || bottom - top < 10.0 {
            return;
        }
        let top_height = self.drop_height.max(0.01);
        let duration = self.duration.max(0.05);
        let x_of = |t: f32| left + (t / duration).clamp(0.0, 1.0) * (right - left);
        let y_of = |height: f32| bottom - (height / top_height).clamp(0.0, 1.05) * (bottom - top);

        // Grid: heights at 0, 1/2 and the drop height; time at each second.
        for frac in [0.5_f32, 1.0] {
            let y = bottom - frac * (bottom - top);
            stroke_polyline(window, &[pt(left, y), pt(right, y)], 1.0, c.grid);
        }
        for second in grid_seconds(duration) {
            let x = x_of(second);
            stroke_polyline(window, &[pt(x, top), pt(x, bottom)], 1.0, c.grid);
        }

        // The curve, sampled finely enough that the arcs look smooth.
        let mut curve: Vec<Point<Pixels>> = Vec::new();
        for arc in &self.arcs {
            let steps = (((arc.end - arc.start) / duration) * (right - left) / 3.0).ceil().clamp(6.0, 90.0) as usize;
            for i in 0..=steps {
                let t = arc.start + (arc.end - arc.start) * i as f32 / steps as f32;
                curve.push(pt(x_of(t), y_of(arc.height_at(t, self.gravity))));
            }
        }
        if curve.len() >= 2 {
            let mut area = curve.clone();
            area.push(pt(curve.last().unwrap().x.as_f32(), bottom));
            area.push(pt(curve[0].x.as_f32(), bottom));
            fill_polygon(window, &area, c.fill);
            stroke_polyline(window, &curve, 2.2, c.line);
        }

        // Where each bounce peaks and where it lands.
        for arc in &self.arcs {
            if !arc.drop {
                let mid = 0.5 * (arc.start + arc.end);
                dot(window, pt(x_of(mid), y_of(arc.peak)), 2.6, c.accent);
            }
            dot(window, pt(x_of(arc.end), bottom), 2.4, c.muted);
        }
        if let Some(first) = self.arcs.first() {
            dot(window, pt(x_of(first.start), y_of(first.peak)), 4.0, c.info);
        }

        // Axes on top.
        stroke_polyline(window, &[pt(left, top), pt(left, bottom), pt(right, bottom)], 1.5, c.axis);
    }
}

/// The times the bounce plot draws a grid line at: every second, every two
/// past 12 s, and wider steps for longer spans so there are never more than
/// a dozen or so. An invalid material (restitution above 1) can make the
/// span astronomically long or infinite; that draws no lines rather than
/// stepping one second at a time towards it.
fn grid_seconds(duration: f32) -> impl Iterator<Item = f32> {
    const MAX_LINES: f32 = 12.0;
    let step = if !duration.is_finite() {
        f32::INFINITY
    } else if duration <= MAX_LINES {
        1.0
    } else if duration <= 2.0 * MAX_LINES {
        2.0
    } else {
        (duration / MAX_LINES).ceil()
    };
    let lines = if step.is_finite() { (duration / step).ceil() as usize } else { 0 };
    (1..lines.min(MAX_LINES as usize + 2))
        .map(move |k| k as f32 * step)
        .filter(move |second| *second < duration)
}

// ── Incline diagram ────────────────────────────────────────────────────────

/// Everything the incline diagram draws.
#[derive(Clone, Copy)]
pub struct SlopePlot {
    pub state: SlopeState,
    pub colors: ChartColors,
}

impl SlopePlot {
    pub fn paint(&self, bounds: Bounds<Pixels>, window: &mut Window) {
        let c = &self.colors;
        let s = &self.state;
        let (ox, oy) = (bounds.origin.x.as_f32(), bounds.origin.y.as_f32());
        let (w, h) = (bounds.size.width.as_f32(), bounds.size.height.as_f32());
        let margin = 16.0;
        let base_y = oy + h - margin;
        let base_x = ox + margin + 6.0;
        let avail_w = (w - margin * 2.0 - 12.0).max(40.0);
        let avail_h = (h - margin * 2.0 - 18.0).max(40.0);

        let theta = s.angle_deg.to_radians();
        let (cos, sin) = (theta.cos().max(0.05), theta.sin());
        // Longest ramp that fits at this angle.
        let length = (avail_w / cos).min(if sin > 0.02 { avail_h / sin } else { f32::MAX }).min(avail_w * 1.1);
        let top = (base_x + length * theta.cos(), base_y - length * sin);
        let foot = (base_x + length * theta.cos(), base_y);

        // Ground and ramp body.
        stroke_polyline(window, &[pt(base_x - 6.0, base_y), pt(base_x + avail_w + 6.0, base_y)], 1.5, c.axis);
        fill_polygon(window, &[pt(base_x, base_y), pt(foot.0, foot.1), pt(top.0, top.1)], c.fill);
        stroke_polyline(window, &[pt(base_x, base_y), pt(top.0, top.1), pt(foot.0, foot.1)], 1.8, c.line);

        // The steepest angle the block would hold on.
        let crit = s.critical_deg.to_radians();
        let reach = length.min(avail_w * 0.85);
        dashed_line(
            window,
            pt(base_x, base_y),
            pt(base_x + reach * crit.cos(), base_y - reach * crit.sin()),
            1.2,
            c.warn.opacity(0.85),
        );

        // Angle arc.
        let arc: Vec<Point<Pixels>> = (0..=16)
            .map(|i| {
                let a = theta * i as f32 / 16.0;
                pt(base_x + 34.0 * a.cos(), base_y - 34.0 * a.sin())
            })
            .collect();
        stroke_polyline(window, &arc, 1.2, c.muted);

        // The block, resting on the slope a little past the middle.
        let (tx, ty) = (theta.cos(), -theta.sin()); // along the slope, up
        let (nx, ny) = (-theta.sin(), -theta.cos()); // away from the slope
        let along = length * 0.58;
        let (px_, py_) = (base_x + tx * along, base_y + ty * along);
        let size_px = 30.0_f32.min(length * 0.22).max(14.0);
        let half = size_px * 0.5;
        let corners = [
            (px_ - tx * half, py_ - ty * half),
            (px_ + tx * half, py_ + ty * half),
            (px_ + tx * half + nx * size_px, py_ + ty * half + ny * size_px),
            (px_ - tx * half + nx * size_px, py_ - ty * half + ny * size_px),
        ];
        let block_color = if s.slipping { c.bad } else { c.good };
        let corner_points: Vec<_> = corners.iter().map(|(x, y)| pt(*x, *y)).collect();
        fill_polygon(window, &corner_points, block_color.opacity(0.35));
        let mut outline = corner_points.clone();
        outline.push(corner_points[0]);
        stroke_polyline(window, &outline, 1.8, block_color);

        // Forces on the block, drawn from its center.
        let center = (px_ + nx * half * 1.0, py_ + ny * half * 1.0);
        let scale = 4.2; // px per m/s²
        let g = 9.806_65_f32;
        arrow(window, center, (center.0, center.1 + g * scale), 2.0, c.text);
        arrow(window, center, (center.0 + nx * s.normal * scale, center.1 + ny * s.normal * scale), 2.0, c.info);
        arrow(
            window,
            center,
            (center.0 + tx * s.resisting * scale, center.1 + ty * s.resisting * scale),
            2.0,
            c.warn,
        );
        if s.slipping {
            let net = s.acceleration * scale;
            arrow(window, center, (center.0 - tx * net, center.1 - ty * net), 2.4, c.bad);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::grid_seconds;

    #[test]
    fn grid_lines_stay_few_whatever_the_span() {
        assert_eq!(grid_seconds(3.5).collect::<Vec<_>>(), [1.0, 2.0, 3.0]);
        assert_eq!(grid_seconds(20.0).collect::<Vec<_>>(), [2.0, 4.0, 6.0, 8.0, 10.0, 12.0, 14.0, 16.0, 18.0]);
        assert!(grid_seconds(1e6).count() <= 13);
        // Restitution above 1: the path grows without bound.
        assert!(grid_seconds(3f32.powi(48)).count() <= 13);
        assert_eq!(grid_seconds(f32::INFINITY).count(), 0);
        assert_eq!(grid_seconds(f32::NAN).count(), 0);
    }
}
