use image::DynamicImage;
use ab_glyph::{FontRef, PxScale};
use image::ImageReader;

use crate::CONFIG;

use super::detection_utils::DetectionResult;



pub fn draw_boxes_on_image(acceptable_detections: &Vec<&DetectionResult>) -> Result<DynamicImage, String> {
    let mut image = ImageReader::open(&CONFIG.image_filename)
    .map_err(|err| format!("Could not open image file: {}", err))?
    .decode()
    .map_err(|err| format!("Could not decode image: {}", err))?;

    // Draw bounding boxes
    for detection in acceptable_detections {
        let box_x = detection.r#box[0];
        let box_y = detection.r#box[1];
        let box_width = detection.r#box[2];
        let box_height = detection.r#box[3];
        for i in 0..CONFIG.output.line_thickness {
            imageproc::drawing::draw_hollow_rect_mut(&mut image,
                imageproc::rect::Rect::at(box_x as i32 + i as i32, box_y as i32 + i as i32)
                .of_size(box_width as u32 - 2*i, box_height as u32 - 2*i), image::Rgba(CONFIG.output.line_color));
        }

        // Draw label
        let label = format!("{} ({}%)", detection.cls_name, detection.conf);
        let font = FontRef::try_from_slice(include_bytes!("../DejaVuSans.ttf")).map_err(|err| format!("Could not load font: {}", err))?;
        let scale: PxScale = PxScale { x: 25.0, y: 25.0 };
        let text_size = imageproc::drawing::text_size(scale.clone(), &font, &label);
        let padding_x = 10;
        let padding_y = 5;
        let text_x = box_x + box_width - text_size.0 - padding_x;
        if (text_size.1 + 2 * padding_y) < box_y {
            imageproc::drawing::draw_filled_rect_mut(&mut image,
                imageproc::rect::Rect::at((box_x + box_width - text_size.0 - 2 * padding_x) as i32, (box_y - text_size.1 - 2 * padding_y) as i32)
                .of_size(text_size.0 + 2 * padding_x, text_size.1 + 2 * padding_y), image::Rgba(CONFIG.output.line_color));
            imageproc::drawing::draw_text_mut(&mut image, image::Rgba(CONFIG.output.text_color), text_x as i32, (box_y - text_size.1 - padding_y) as i32, scale, &font, label.as_str());
        } else {
        }
    }

    Ok(image)
}