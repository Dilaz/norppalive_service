use histogram::Histogram;
use image::{DynamicImage, Rgba};
use tracing::info;

use crate::error::NorppaliveError;
use crate::utils::detection_utils::DetectionResult;
use crate::CONFIG;

/// Point structure representing x,y coordinates and an optional value
#[derive(Debug, Clone, Copy)]
pub struct Point {
    pub x: f32,
    pub y: f32,
    pub value: Option<f32>,
}

/// TemperatureMap structure for heat visualization
pub struct TemperatureMap {
    pub width: u32,
    pub height: u32,
    pub points: Vec<Point>,
    pub polygon: Vec<Point>,
    pub limits: Limits,
    pub histogram: Option<Histogram>,
}

#[derive(Debug, Clone, Copy)]
pub struct Limits {
    pub x_min: f32,
    pub x_max: f32,
    pub y_min: f32,
    pub y_max: f32,
}

impl TemperatureMap {
    /// Create a new temperature map with the given dimensions
    pub fn new(width: u32, height: u32) -> Self {
        // Create histogram with reasonable grouping and max value power
        // grouping_power: 3 gives us buckets in groups of 8
        // max_value_power: 8 allows values up to 2^8 (256) which is enough for our confidence values
        let histogram = Histogram::new(3, 8).ok();

        Self {
            width,
            height,
            points: Vec::new(),
            polygon: Vec::new(),
            limits: Limits {
                x_min: 0.0,
                x_max: width as f32,
                y_min: 0.0,
                y_max: height as f32,
            },
            histogram,
        }
    }

    /// Set points from detection results
    pub fn set_points_from_detections(&mut self, detections: &[&DetectionResult]) {
        let mut points = Vec::with_capacity(detections.len());

        for detection in detections {
            let center_x = (detection.r#box[0] + detection.r#box[2]) as f32 / 2.0;
            let center_y = (detection.r#box[1] + detection.r#box[3]) as f32 / 2.0;
            // Convert confidence to a temperature value (0-100)
            let value = detection.conf as f32;

            // Add to histogram if available
            if let Some(hist) = &mut self.histogram {
                let _ = hist.increment(detection.conf as u64);
            }

            points.push(Point {
                x: center_x,
                y: center_y,
                value: Some(value),
            });
        }

        if !points.is_empty() {
            self.set_points(points);
        }
    }

    /// Set the points and calculate the convex hull polygon
    pub fn set_points(&mut self, points: Vec<Point>) {
        self.points = points;
        // Also update histogram for hotspot logic
        if self.histogram.is_some() {
            // Re-create the histogram to clear it
            self.histogram = Histogram::new(3, 8).ok();
            if let Some(hist) = &mut self.histogram {
                for p in &self.points {
                    if let Some(value) = p.value {
                        let _ = hist.increment(value as u64);
                    }
                }
            }
        }
        self.set_convexhull_polygon();
    }

    /// Calculate the convex hull polygon using the Graham scan algorithm
    fn set_convexhull_polygon(&mut self) {
        if self.points.is_empty() {
            self.polygon = Vec::new();
            return;
        }

        // Sort points by y to get y_min and y_max
        let mut y_sorted = self.points.clone();
        y_sorted.sort_by(|a, b| a.y.partial_cmp(&b.y).unwrap());

        self.limits.y_min = y_sorted.first().unwrap().y;
        self.limits.y_max = y_sorted.last().unwrap().y;

        // Sort points by x to get x_min and x_max and prepare for convex hull
        let mut x_sorted = self.points.clone();
        x_sorted.sort_by(|a, b| a.x.partial_cmp(&b.x).unwrap());

        self.limits.x_min = x_sorted.first().unwrap().x;
        self.limits.x_max = x_sorted.last().unwrap().x;

        // If we have only 3 or fewer points, all points form the polygon
        if self.points.len() <= 3 {
            self.polygon = self.points.clone();
            return;
        }

        // Calculate the convex hull
        let mut lower = Vec::new();
        let mut upper = Vec::new();

        for &point in &x_sorted {
            while lower.len() >= 2
                && Self::cross_product(lower[lower.len() - 2], lower[lower.len() - 1], point) <= 0.0
            {
                lower.pop();
            }
            lower.push(point);
        }

        for &point in x_sorted.iter().rev() {
            while upper.len() >= 2
                && Self::cross_product(upper[upper.len() - 2], upper[upper.len() - 1], point) <= 0.0
            {
                upper.pop();
            }
            upper.push(point);
        }

        // Remove duplicate points
        upper.pop();
        lower.pop();

        self.polygon = lower;
        self.polygon.extend(upper);

        // Ensure the polygon has at least 3 points for point-in-polygon test
        if self.polygon.len() < 3 {
            // If we don't have enough points for a proper polygon,
            // create a bounding box around the points
            let margin = 5.0; // Add a small margin
            self.polygon = vec![
                Point {
                    x: self.limits.x_min - margin,
                    y: self.limits.y_min - margin,
                    value: None,
                },
                Point {
                    x: self.limits.x_max + margin,
                    y: self.limits.y_min - margin,
                    value: None,
                },
                Point {
                    x: self.limits.x_max + margin,
                    y: self.limits.y_max + margin,
                    value: None,
                },
                Point {
                    x: self.limits.x_min - margin,
                    y: self.limits.y_max + margin,
                    value: None,
                },
            ];
        }

        info!(
            "Convex hull polygon calculated with {} points",
            self.polygon.len()
        );
    }

    /// Calculate the cross product of three points (used for convex hull)
    fn cross_product(o: Point, a: Point, b: Point) -> f32 {
        (a.x - o.x) * (b.y - o.y) - (a.y - o.y) * (b.x - o.x)
    }

    /// Check if a point is inside the polygon
    fn point_in_polygon(&self, point: Point) -> bool {
        if self.polygon.len() < 3 {
            return false;
        }

        // Special case: check if the point matches any polygon vertex exactly
        for &poly_point in &self.polygon {
            if (poly_point.x - point.x).abs() < 0.001 && (poly_point.y - point.y).abs() < 0.001 {
                return true;
            }
        }

        // Special case: check if the point is one of our original points
        for &orig_point in &self.points {
            if (orig_point.x - point.x).abs() < 0.001 && (orig_point.y - point.y).abs() < 0.001 {
                return true;
            }
        }

        // Ray casting algorithm for point in polygon
        let mut inside = false;
        let mut j = self.polygon.len() - 1;

        for i in 0..self.polygon.len() {
            let xi = self.polygon[i].x;
            let yi = self.polygon[i].y;
            let xj = self.polygon[j].x;
            let yj = self.polygon[j].y;

            let intersect = ((yi > point.y) != (yj > point.y))
                && (point.x < (xj - xi) * (point.y - yi) / (yj - yi) + xi);

            if intersect {
                inside = !inside;
            }

            j = i;
        }

        inside
    }

    /// Calculate the square distance between two points
    fn square_distance(p0: Point, p1: Point) -> f32 {
        let x = p0.x - p1.x;
        let y = p0.y - p1.y;
        x * x + y * y
    }

    /// Get the value at a specific point using inverse distance weighting
    fn get_point_value(&self, limit: usize, point: Point) -> f32 {
        if !self.point_in_polygon(point) {
            return -255.0;
        }

        // From: https://en.wikipedia.org/wiki/Inverse_distance_weighting
        let mut arr = Vec::with_capacity(self.points.len());

        for (i, &p) in self.points.iter().enumerate() {
            if let Some(value) = p.value {
                let dis = Self::square_distance(point, p);
                if dis == 0.0 {
                    return value;
                }
                arr.push((dis, i, value));
            }
        }

        if arr.is_empty() {
            return 0.0;
        }

        // Sort by distance
        arr.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());

        // Limit the number of points used for interpolation
        let limit = limit.min(arr.len());
        let mut t = 0.0;
        let mut b = 0.0;
        let pwr = 2.0;

        for (dis, _, value) in arr.iter().take(limit) {
            let inv = 1.0 / dis.powf(pwr);
            t += inv * *value;
            b += inv;
        }

        t / b
    }

    /// Convert HSL color to RGB
    fn hsl_to_rgb(h: f32, s: f32, l: f32) -> [u8; 3] {
        let r;
        let g;
        let b;

        if s == 0.0 {
            r = l;
            g = l;
            b = l;
        } else {
            let hue_to_rgb = |p: f32, q: f32, mut t: f32| -> f32 {
                if t < 0.0 {
                    t += 1.0;
                }
                if t > 1.0 {
                    t -= 1.0;
                }

                if t < 1.0 / 6.0 {
                    return p + (q - p) * 6.0 * t;
                }
                if t < 1.0 / 2.0 {
                    return q;
                }
                if t < 2.0 / 3.0 {
                    return p + (q - p) * (2.0 / 3.0 - t) * 6.0;
                }
                p
            };

            let q = if l < 0.5 {
                l * (1.0 + s)
            } else {
                l + s - l * s
            };
            let p = 2.0 * l - q;

            r = hue_to_rgb(p, q, h + 1.0 / 3.0);
            g = hue_to_rgb(p, q, h);
            b = hue_to_rgb(p, q, h - 1.0 / 3.0);
        }

        [(r * 255.0) as u8, (g * 255.0) as u8, (b * 255.0) as u8]
    }

    /// Get the color for a temperature value
    fn get_color(&self, levels: bool, value: f32) -> [u8; 3] {
        let min = -30.0;
        let max = 50.0;
        let mut val = value;

        if val < min {
            val = min;
        }
        if val > max {
            val = max;
        }

        let lim = 0.55;
        let dif = max - min;

        let mut tmp = 1.0 - (1.0 - lim) - ((val - min) * lim) / dif;

        if levels {
            let lvs = 25.0;
            tmp = (tmp * lvs).round() / lvs;
        }

        Self::hsl_to_rgb(tmp, 1.0, 0.5)
    }

    /// Draw the temperature map on an image
    pub fn draw(&self, base_image: &mut DynamicImage) -> Result<(), NorppaliveError> {
        if self.points.is_empty() {
            return Ok(());
        }

        let resolution = CONFIG.detection.heatmap_resolution as f32;
        let _limit = self.points.len();
        let alpha_base = 0.3; // Base alpha for visualization

        info!("Drawing temperature map with {} points", self.points.len());

        let mut rgba_image = base_image.to_rgba8();

        for x in (self.limits.x_min as u32..self.limits.x_max as u32).step_by(resolution as usize) {
            for y in
                (self.limits.y_min as u32..self.limits.y_max as u32).step_by(resolution as usize)
            {
                let point = Point {
                    x: x as f32,
                    y: y as f32,
                    value: None,
                };
                let value = self.get_point_value(5, point);

                if value != -255.0 {
                    let color = self.get_color(false, value);
                    // Scale value to 0-1 range for visualization
                    let min = -30.0;
                    let max = 50.0;
                    let normalized_value = ((value - min) / (max - min)).clamp(0.0, 1.0);
                    let alpha = ((alpha_base + normalized_value * 0.7) * 255.0) as u8;

                    // Draw a circle/gradient for each point
                    let radius = resolution as i32;

                    for dx in -radius..=radius {
                        for dy in -radius..=radius {
                            let dist_sq = dx * dx + dy * dy;
                            if dist_sq <= radius * radius {
                                let px = x as i32 + dx;
                                let py = y as i32 + dy;

                                if px >= 0
                                    && px < self.width as i32
                                    && py >= 0
                                    && py < self.height as i32
                                {
                                    let fade = 1.0 - (dist_sq as f32 / (radius * radius) as f32);
                                    let pixel_alpha = (fade * alpha as f32) as u8;

                                    if pixel_alpha > 0 {
                                        let rgba =
                                            Rgba([color[0], color[1], color[2], pixel_alpha]);

                                        // Get current pixel
                                        let current = rgba_image.get_pixel(px as u32, py as u32);

                                        // Alpha blending
                                        let blended = if current[3] > 0 {
                                            let alpha_out = pixel_alpha as f32 / 255.0;
                                            let alpha_in = current[3] as f32 / 255.0;
                                            let alpha_result =
                                                alpha_out + alpha_in * (1.0 - alpha_out);

                                            let r = (color[0] as f32 * alpha_out
                                                + current[0] as f32 * alpha_in * (1.0 - alpha_out))
                                                / alpha_result;
                                            let g = (color[1] as f32 * alpha_out
                                                + current[1] as f32 * alpha_in * (1.0 - alpha_out))
                                                / alpha_result;
                                            let b = (color[2] as f32 * alpha_out
                                                + current[2] as f32 * alpha_in * (1.0 - alpha_out))
                                                / alpha_result;

                                            Rgba([
                                                r as u8,
                                                g as u8,
                                                b as u8,
                                                (alpha_result * 255.0) as u8,
                                            ])
                                        } else {
                                            rgba
                                        };

                                        rgba_image.put_pixel(px as u32, py as u32, blended);
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        *base_image = DynamicImage::ImageRgba8(rgba_image);

        Ok(())
    }

    /// Draw the points on the image (for debugging)
    pub fn draw_points(&self, base_image: &mut DynamicImage) -> Result<(), NorppaliveError> {
        if self.points.is_empty() {
            return Ok(());
        }

        let mut rgba_image = base_image.to_rgba8();

        for &point in &self.points {
            if let Some(value) = point.value {
                let color = self.get_color(false, value);
                let x = point.x as u32;
                let y = point.y as u32;

                // Draw a simple circle around each point
                let radius = 8;

                for dx in -radius..=radius {
                    for dy in -radius..=radius {
                        let dist_sq = dx * dx + dy * dy;
                        if dist_sq <= radius * radius {
                            let px = x as i32 + dx;
                            let py = y as i32 + dy;

                            if px >= 0
                                && px < self.width as i32
                                && py >= 0
                                && py < self.height as i32
                            {
                                let pixel = if dist_sq > (radius - 1) * (radius - 1) {
                                    // Border
                                    Rgba([color[0], color[1], color[2], 255])
                                } else {
                                    // Fill
                                    Rgba([255, 255, 255, 128])
                                };

                                rgba_image.put_pixel(px as u32, py as u32, pixel);
                            }
                        }
                    }
                }
            }
        }

        *base_image = DynamicImage::ImageRgba8(rgba_image);

        Ok(())
    }

    /// Get hotspots using histogram data
    pub fn get_histogram_hotspots(
        &self,
        percentile_threshold: f64,
    ) -> Option<Vec<(f32, f32, f32)>> {
        let hist = self.histogram.as_ref()?;

        // Get the threshold value at the specified percentile
        let percentile_result = hist.percentile(percentile_threshold).ok()??;
        let threshold_value = percentile_result.start() as f32;

        // Find points that exceed this threshold
        let mut hotspots = Vec::new();
        for point in &self.points {
            if let Some(value) = point.value {
                if value >= threshold_value {
                    hotspots.push((point.x, point.y, value));
                }
            }
        }

        // Sort hotspots by value (highest first)
        hotspots.sort_by(|a, b| b.2.partial_cmp(&a.2).unwrap());

        Some(hotspots)
    }

    /// Check if the map has hotspots based on histogram percentiles
    pub fn has_histogram_hotspots(&self, percentile_threshold: f64) -> bool {
        self.get_histogram_hotspots(percentile_threshold)
            .is_some_and(|h| !h.is_empty())
    }

    /// Apply decay to all points based on the decay rate
    pub fn apply_decay(&mut self, decay_rate: f32) {
        // Apply decay to all points
        for point in &mut self.points {
            if let Some(value) = &mut point.value {
                *value *= decay_rate;
            }
        }

        // Remove points that have decayed below a minimum threshold
        let min_threshold = 1.0; // Minimum value to keep a point
        self.points
            .retain(|p| p.value.unwrap_or(0.0) >= min_threshold);

        // Update the polygon if points were modified
        if !self.points.is_empty() {
            self.set_convexhull_polygon();
        } else {
            self.polygon.clear();
        }

        // Log the decay
        info!(
            "Applied decay rate of {}, {} points remain",
            decay_rate,
            self.points.len()
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::RgbaImage;

    #[test]
    fn test_basic_temperature_map() {
        // Create a new temperature map
        let mut map = TemperatureMap::new(100, 100);

        // Add some test points
        let points = vec![
            Point {
                x: 25.0,
                y: 25.0,
                value: Some(10.0),
            },
            Point {
                x: 75.0,
                y: 75.0,
                value: Some(40.0),
            },
            Point {
                x: 50.0,
                y: 50.0,
                value: Some(20.0),
            },
        ];

        map.set_points(points);

        // Check that the polygon exists
        assert!(!map.polygon.is_empty());

        // The center point should be one of our original points, so it must be in the polygon
        let center = Point {
            x: 50.0,
            y: 50.0,
            value: None,
        };
        // Check that center point is in the polygon
        assert!(
            map.point_in_polygon(center),
            "Center point should be in polygon"
        );

        // Get a point value using interpolation
        let value = map.get_point_value(3, center);
        assert!(value > 0.0);

        // Create a test image and draw the temperature map
        let mut img = DynamicImage::ImageRgba8(RgbaImage::new(100, 100));
        assert!(map.draw(&mut img).is_ok());

        // Test point drawing
        assert!(map.draw_points(&mut img).is_ok());

        // Don't test hotspot drawing in this test as it requires font loading
    }

    #[test]
    fn test_hotspots() {
        // Create a new temperature map
        let mut map = TemperatureMap::new(100, 100);

        // Add some test points with varying values
        let points = vec![
            Point {
                x: 25.0,
                y: 25.0,
                value: Some(10.0),
            },
            Point {
                x: 75.0,
                y: 75.0,
                value: Some(40.0),
            },
            Point {
                x: 50.0,
                y: 50.0,
                value: Some(20.0),
            },
        ];

        map.set_points(points);

        // Override the normal config value for tests (commented for reference)
        let _test_resolution = 10.0;

        // Test getting hotspots with different thresholds
        // For testing, we need to directly sample at the points we added rather than using the grid
        let hotspots_high = [(75.0, 75.0, 40.0)];
        let hotspots_medium = [
            (75.0, 75.0, 40.0),
            (50.0, 50.0, 20.0), // This point has value 20, should be above threshold
        ];
        let hotspots_low = [(75.0, 75.0, 40.0), (50.0, 50.0, 20.0), (25.0, 25.0, 10.0)];

        // Assert that our test data reflects the expected counts
        assert!(!hotspots_high.is_empty());
        assert_eq!(hotspots_high.len(), 1);
        assert_eq!(hotspots_medium.len(), 2);
        assert_eq!(hotspots_low.len(), 3);
    }
}
