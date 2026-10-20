//! Textures no larger than the GPU takes.
//!
//! The glow painter asserts that a texture is at most `max_texture_side`
//! texels on each side, and release builds abort on a panic. An image a
//! Luna plugin or theme shows (a `data:` URL, an `<img>`, an SVG with a
//! large `width`, a wide canvas or video) could otherwise close the app,
//! again at every start while the plugin or theme stays installed. Images
//! loaded by URI go through [`Loader`], which scales one that is too large
//! before it becomes a texture; code that makes its own textures from a
//! plugin's pixels passes them through [`fit`].

use std::collections::HashMap;
use std::sync::Mutex;

use egui::load::{ImagePoll, LoadError, SizeHint, SizedTexture, TextureLoadResult, TexturePoll};
use egui::{Color32, ColorImage, TextureHandle, TextureOptions};

/// `image`, scaled down (nearest texel) when a side is over the most the GPU
/// takes; unchanged otherwise.
pub fn fit(ctx: &egui::Context, image: ColorImage) -> ColorImage {
    fit_to(ctx.input(|i| i.max_texture_side), image)
}

fn fit_to(most: usize, image: ColorImage) -> ColorImage {
    let [w, h] = image.size;
    if w <= most && h <= most || most == 0 {
        return image;
    }
    let scale = most as f64 / w.max(h) as f64;
    let nw = ((w as f64 * scale).round() as usize).clamp(1, most);
    let nh = ((h as f64 * scale).round() as usize).clamp(1, most);
    let mut pixels = Vec::with_capacity(nw * nh);
    for y in 0..nh {
        let sy = (y * h / nh).min(h - 1);
        for x in 0..nw {
            let sx = (x * w / nw).min(w - 1);
            pixels.push(
                image
                    .pixels
                    .get(sy * w + sx)
                    .copied()
                    .unwrap_or(Color32::TRANSPARENT),
            );
        }
    }
    let mut out = ColorImage::new([nw, nh], pixels);
    out.source_size = image.source_size;
    out
}

/// A texture made from an image scaled to fit: its options, the SVG size it
/// was drawn at, the texture and the image's own size.
type Scaled = (TextureOptions, Option<SizeHint>, TextureHandle, egui::Vec2);

#[derive(Default)]
struct State {
    /// Images found to fit (an SVG per size asked for): egui's loader has
    /// them, and they are not looked at again.
    fits: HashMap<String, Vec<Option<SizeHint>>>,
    scaled: HashMap<String, Vec<Scaled>>,
}

/// A texture loader that takes only images too large for the GPU, scaled
/// to fit; every other image goes on to egui's own loader. Installed after
/// it, so it is asked first. A known image costs a lookup and no
/// allocation.
#[derive(Default)]
pub struct Loader {
    state: Mutex<State>,
}

/// An SVG is drawn at the size asked for, so that size can make it too
/// large where a smaller one fitted.
fn svg_hint(uri: &str, size_hint: SizeHint) -> Option<SizeHint> {
    let b = uri.as_bytes();
    let svg = (b.len() >= 4 && b[b.len() - 4..].eq_ignore_ascii_case(b".svg"))
        || (b.len() >= 14 && b[..14].eq_ignore_ascii_case(b"data:image/svg"));
    svg.then_some(size_hint)
}

impl egui::load::TextureLoader for Loader {
    fn id(&self) -> &'static str {
        "tidalfast::texture_fit::Loader"
    }

    fn load(
        &self,
        ctx: &egui::Context,
        uri: &str,
        texture_options: TextureOptions,
        size_hint: SizeHint,
    ) -> TextureLoadResult {
        let hint = svg_hint(uri, size_hint);
        let mut state = self.state.lock().unwrap_or_else(|p| p.into_inner());
        if state.fits.get(uri).is_some_and(|h| h.contains(&hint)) {
            return Err(LoadError::NotSupported);
        }
        if let Some((.., handle, size)) = state.scaled.get(uri).and_then(|v| {
            v.iter()
                .find(|(o, h, ..)| *o == texture_options && *h == hint)
        }) {
            return Ok(TexturePoll::Ready {
                texture: SizedTexture::new(handle.id(), *size),
            });
        }
        let image = match ctx.try_load_image(uri, size_hint)? {
            ImagePoll::Pending { size } => return Ok(TexturePoll::Pending { size }),
            ImagePoll::Ready { image } => image,
        };
        let most = ctx.input(|i| i.max_texture_side);
        if image.size[0] <= most && image.size[1] <= most {
            state.fits.entry(uri.to_string()).or_default().push(hint);
            return Err(LoadError::NotSupported);
        }
        let size = image.source_size;
        let handle = ctx.load_texture(uri, fit_to(most, (*image).clone()), texture_options);
        state.scaled.entry(uri.to_string()).or_default().push((
            texture_options,
            hint,
            handle.clone(),
            size,
        ));
        Ok(TexturePoll::Ready {
            texture: SizedTexture::new(handle.id(), size),
        })
    }

    fn forget(&self, uri: &str) {
        let mut state = self.state.lock().unwrap_or_else(|p| p.into_inner());
        state.fits.remove(uri);
        state.scaled.remove(uri);
    }

    fn forget_all(&self) {
        *self.state.lock().unwrap_or_else(|p| p.into_inner()) = State::default();
    }

    fn byte_size(&self) -> usize {
        self.state
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .scaled
            .values()
            .flatten()
            .map(|(_, _, h, _)| h.byte_size())
            .sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_large_image_is_scaled_to_the_largest_side_and_a_small_one_kept() {
        let wide = ColorImage::filled([5000, 4], Color32::RED);
        let out = fit_to(2048, wide);
        assert_eq!(out.size, [2048, 2]);
        assert_eq!(out.pixels.len(), 2048 * 2);
        assert_eq!(out.source_size, egui::vec2(5000.0, 4.0));
        let tall = fit_to(100, ColorImage::filled([1, 100_000], Color32::RED));
        assert_eq!(tall.size, [1, 100]);
        let small = ColorImage::filled([30, 20], Color32::BLUE);
        assert_eq!(fit_to(2048, small.clone()), small);
    }
}
