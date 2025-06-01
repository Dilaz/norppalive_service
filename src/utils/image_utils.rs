use ab_glyph::{FontRef, PxScale};
use image::DynamicImage;
use image::ImageReader;
use miette::Result;

use crate::{config::CONFIG, error::NorppaliveError};

use super::detection_utils::DetectionResult;

pub fn draw_boxes_on_image(
    acceptable_detections: &[&DetectionResult],
) -> Result<DynamicImage, NorppaliveError> {
    let mut image = ImageReader::open(&CONFIG.image_filename)?.decode()?;

    // Draw bounding boxes
    for detection in acceptable_detections {
        let box_x = detection.r#box[0];
        let box_y = detection.r#box[1];
        let box_width = detection.r#box[2] - detection.r#box[0];
        let box_height = detection.r#box[3] - detection.r#box[1];
        for i in 0..CONFIG.output.line_thickness {
            imageproc::drawing::draw_hollow_rect_mut(
                &mut image,
                imageproc::rect::Rect::at(box_x as i32 + i as i32, box_y as i32 + i as i32)
                    .of_size(box_width - 2 * i, box_height - 2 * i),
                image::Rgba(CONFIG.output.line_color),
            );
        }

        // Draw label
        let label = format!("{} ({}%)", detection.cls_name, detection.conf);
        let font = FontRef::try_from_slice(include_bytes!("../DejaVuSans.ttf"))?;
        let scale: PxScale = PxScale { x: 25.0, y: 25.0 };
        let text_size = imageproc::drawing::text_size(scale, &font, &label);
        let padding_x = 10;
        let padding_y = 5;

        // Ensure we don't have overflow by checking bounds
        let text_width = text_size.0 + 2 * padding_x;
        let text_height = text_size.1 + 2 * padding_y;

        // Calculate text position with overflow protection
        let text_x = if box_width > text_size.0 + padding_x {
            box_x + box_width - text_size.0 - padding_x
        } else {
            box_x
        };

        if text_height < box_y {
            // Draw text above the bounding box
            imageproc::drawing::draw_filled_rect_mut(
                &mut image,
                imageproc::rect::Rect::at(
                    (if box_width > text_width {
                        box_x + box_width - text_width
                    } else {
                        box_x
                    }) as i32,
                    (box_y - text_height) as i32,
                )
                .of_size(text_width, text_height),
                image::Rgba(CONFIG.output.line_color),
            );
            imageproc::drawing::draw_text_mut(
                &mut image,
                image::Rgba(CONFIG.output.text_color),
                text_x as i32,
                (box_y - text_height + padding_y) as i32,
                scale,
                &font,
                label.as_str(),
            );
        } else {
            // Draw text below the bounding box when detection is too high
            let text_y = box_y + box_height;

            // Ensure we don't draw beyond image boundaries
            if text_y + text_height <= image.height() {
                imageproc::drawing::draw_filled_rect_mut(
                    &mut image,
                    imageproc::rect::Rect::at(
                        (if box_width > text_width {
                            box_x + box_width - text_width
                        } else {
                            box_x
                        }) as i32,
                        text_y as i32,
                    )
                    .of_size(text_width, text_height),
                    image::Rgba(CONFIG.output.line_color),
                );
                imageproc::drawing::draw_text_mut(
                    &mut image,
                    image::Rgba(CONFIG.output.text_color),
                    text_x as i32,
                    (text_y + padding_y) as i32,
                    scale,
                    &font,
                    label.as_str(),
                );
            }
        }
    }

    Ok(image)
}
