use ipld_core::ipld::Ipld;
use std::collections::BTreeMap;
use std::path::Path;
use tracing::warn;

fn aspect_ratio_ipld(width: u32, height: u32) -> Option<Ipld> {
    if width == 0 || height == 0 {
        return None;
    }
    Some(Ipld::Map(BTreeMap::from([
        ("width".to_string(), Ipld::Integer(width as i128)),
        ("height".to_string(), Ipld::Integer(height as i128)),
    ])))
}

// Reads image dimensions and returns an `aspectRatio` Ipld map suitable for
// `app.bsky.embed.images#image`. Without this field, Bluesky clients render a
// default placeholder shape that letterboxes the actual image. Returns `None`
// (with a warning) on read failure rather than failing the whole post.
pub fn image_aspect_ratio_ipld(image_path: &str) -> Option<Ipld> {
    match image::image_dimensions(image_path) {
        Ok((w, h)) => aspect_ratio_ipld(w, h),
        Err(e) => {
            warn!(
                "Could not read image dimensions for {}: {} \u{2014} omitting aspectRatio",
                image_path, e
            );
            None
        }
    }
}

// Probes a video file for its display dimensions and returns an `aspectRatio`
// Ipld map for `app.bsky.embed.video`. Uses ffmpeg-next to read the first
// video stream's width/height, accounting for any non-square pixel aspect
// ratio (SAR) so the displayed frame matches what the player will render.
pub fn video_aspect_ratio_ipld(path: &Path) -> Option<Ipld> {
    let ctx = match ffmpeg_next::format::input(&path) {
        Ok(c) => c,
        Err(e) => {
            warn!(
                "Could not open video {} for aspect-ratio probe: {} \u{2014} omitting aspectRatio",
                path.display(),
                e
            );
            return None;
        }
    };
    let stream = ctx
        .streams()
        .best(ffmpeg_next::media::Type::Video)
        .or_else(|| ctx.streams().find(|s| s.parameters().medium() == ffmpeg_next::media::Type::Video))?;
    let decoder = match ffmpeg_next::codec::context::Context::from_parameters(stream.parameters())
        .and_then(|c| c.decoder().video())
    {
        Ok(d) => d,
        Err(e) => {
            warn!(
                "Could not decode video parameters for {}: {} \u{2014} omitting aspectRatio",
                path.display(),
                e
            );
            return None;
        }
    };
    let (raw_w, raw_h) = (decoder.width(), decoder.height());
    let sar = decoder.aspect_ratio();
    // SAR adjusts pixel shape; apply it to width so the ratio matches what
    // the player displays. SAR of 0/1 (unknown) is treated as 1:1.
    let (w, h) = if sar.numerator() > 0 && sar.denominator() > 0 {
        let adjusted_w =
            (raw_w as i64 * sar.numerator() as i64 / sar.denominator() as i64).max(1) as u32;
        (adjusted_w, raw_h)
    } else {
        (raw_w, raw_h)
    };
    aspect_ratio_ipld(w, h)
}
