//! The Featured slideshow at the top of Explore: big slides, each against its app's
//! icon color, that move along on their own every few seconds.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::rc::{Rc, Weak};
use std::time::Duration;

use adw::prelude::*;
use gtk::{gdk, glib};

use super::{Change, Store, icons, settings_page};
use crate::index::{self, App, Pick};

const PLACEHOLDER: &str = "outpost-application-x-executable-symbolic";

/// Shown behind apps whose icons are grey, or still loading.
const NEUTRAL: &str = "#3d3846";

const CSS: &str = "
.outpost-slide {
  background-color: #3d3846;
  background-image: none;
  box-shadow: none;
  border-radius: 12px;
  color: white;
  padding: 24px;
  transition: background-color 400ms ease, filter 150ms ease;
}
.outpost-slide:hover { filter: brightness(1.1); }
.outpost-slide:active { filter: brightness(0.95); }
.outpost-slide .outpost-badge {
  background-color: rgba(255, 255, 255, 0.18);
  border-radius: 999px;
  padding: 3px 12px;
  font-weight: bold;
  font-size: 0.85em;
}
.outpost-slide .outpost-summary { opacity: 0.85; }
.outpost-slide-edge,
.outpost-slide-edge:hover,
.outpost-slide-edge:active {
  background: none;
  box-shadow: none;
  padding: 0;
  min-width: 0;
}
.outpost-slide-edge:hover > button { filter: brightness(1.6); }
.outpost-slide-edge:active > button { filter: brightness(2); }
";

thread_local! {
    /// Each icon's slide color, as a CSS class name and its rule.
    static COLORS: RefCell<HashMap<String, String>> = RefCell::default();
    static COLOR_CSS: gtk::CssProvider = gtk::CssProvider::new();
    static CSS_ADDED: Cell<bool> = const { Cell::new(false) };
}

pub fn build(store: &Rc<Store>) -> gtk::Box {
    add_css();
    let carousel = adw::Carousel::builder()
        .spacing(12)
        .allow_long_swipes(false)
        // Otherwise scrolling down the page would flick through slides instead.
        .allow_scroll_wheel(false)
        .build();
    let previous = arrow("outpost-go-previous-symbolic", "Previous", gtk::Align::Start);
    let next = arrow("outpost-go-next-symbolic", "Next", gtk::Align::End);
    let overlay = gtk::Overlay::builder().child(&carousel).build();
    overlay.add_overlay(&previous);
    overlay.add_overlay(&next);

    let pause = gtk::Button::builder().css_classes(["flat", "circular"]).build();
    let controls = gtk::CenterBox::new();
    controls.set_center_widget(Some(&adw::CarouselIndicatorDots::builder().carousel(&carousel).build()));
    controls.set_end_widget(Some(&pause));

    let section = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(6)
        .build();
    section.append(&overlay);
    section.append(&controls);

    let settings = settings_page::current_settings();
    let show = Rc::new(Slideshow {
        store: Rc::downgrade(store),
        carousel: carousel.downgrade(),
        arrows: [previous.clone(), next.clone()],
        pause: pause.clone(),
        section: section.clone(),
        timer: RefCell::default(),
        seconds: Cell::new(settings.slide_seconds()),
        count: Cell::new(settings.recommendations()),
        paused: Cell::new(settings.slideshow_paused),
    });
    show.fill();
    show.show_paused();
    show.restart();

    {
        let show = show.clone();
        previous.connect_clicked(move |_| show.step(-1));
    }
    {
        let show = show.clone();
        next.connect_clicked(move |_| show.step(1));
    }
    {
        let show = show.clone();
        // Moving by hand restarts the countdown, so the next slide gets its full time.
        carousel.connect_page_changed(move |_, _| show.restart());
    }
    {
        let store = store.clone();
        pause.connect_clicked(move |_| settings_page::change_settings(&store, |s| s.slideshow_paused = !s.slideshow_paused));
    }
    store.watch(move |change| {
        if show.carousel.upgrade().is_none() {
            return false;
        }
        if change == Change::Settings {
            show.reload_settings();
        }
        true
    });
    section
}

struct Slideshow {
    store: Weak<Store>,
    carousel: glib::WeakRef<adw::Carousel>,
    arrows: [gtk::Button; 2],
    pause: gtk::Button,
    section: gtk::Box,
    timer: RefCell<Option<glib::SourceId>>,
    seconds: Cell<u32>,
    count: Cell<usize>,
    paused: Cell<bool>,
}

impl Slideshow {
    fn fill(&self) {
        let (Some(store), Some(carousel)) = (self.store.upgrade(), self.carousel.upgrade()) else { return };
        while carousel.n_pages() > 0 {
            carousel.remove(&carousel.nth_page(0));
        }
        for (app, pick) in index::recommendations(&store.apps(), self.count.get()) {
            carousel.append(&slide(&store, &app, pick));
        }
        let pages = carousel.n_pages();
        self.section.set_visible(pages > 0);
        for arrow in &self.arrows {
            arrow.set_visible(pages > 1);
        }
        self.pause.set_visible(pages > 1);
    }

    fn reload_settings(self: &Rc<Self>) {
        let settings = settings_page::current_settings();
        self.seconds.set(settings.slide_seconds());
        self.paused.set(settings.slideshow_paused);
        if self.count.replace(settings.recommendations()) != settings.recommendations() {
            self.fill();
        }
        self.show_paused();
        self.restart();
    }

    fn show_paused(&self) {
        let (icon, tooltip) = if self.paused.get() {
            ("outpost-media-playback-start-symbolic", "Play Slideshow")
        } else {
            ("outpost-media-playback-pause-symbolic", "Pause Slideshow")
        };
        self.pause.set_icon_name(icon);
        self.pause.set_tooltip_text(Some(tooltip));
    }

    /// Moves `by` slides, wrapping around at either end.
    fn step(&self, by: i32) {
        let Some(carousel) = self.carousel.upgrade() else { return };
        let pages = carousel.n_pages() as i32;
        if pages < 2 {
            return;
        }
        let to = (carousel.position().round() as i32 + by).rem_euclid(pages);
        carousel.scroll_to(&carousel.nth_page(to as u32), true);
    }

    /// Starts counting down to the next slide again.
    fn restart(self: &Rc<Self>) {
        if let Some(timer) = self.timer.take() {
            timer.remove();
        }
        if self.paused.get() {
            return;
        }
        let this = Rc::downgrade(self);
        let timer = glib::timeout_add_local_once(Duration::from_secs(self.seconds.get().into()), move || {
            let Some(this) = this.upgrade() else { return };
            this.timer.take();
            let Some(carousel) = this.carousel.upgrade() else { return };
            // Only move along while it's on screen; otherwise wait another round.
            if carousel.is_mapped() {
                this.step(1);
            }
            this.restart();
        });
        self.timer.replace(Some(timer));
    }
}

/// The arrow on one side. The round arrow is only what it looks like: the whole
/// strip from the slide's edge to just past it, top to bottom, is the button, so
/// it's hard to miss and open the app by accident.
fn arrow(icon: &str, tooltip: &str, side: gtk::Align) -> gtk::Button {
    let look = gtk::Button::builder()
        .icon_name(icon)
        .halign(gtk::Align::Center)
        .valign(gtk::Align::Center)
        .can_target(false)
        .focusable(false)
        .css_classes(["circular", "osd"])
        .build();
    gtk::Button::builder()
        .child(&look)
        .tooltip_text(tooltip)
        .halign(side)
        .valign(gtk::Align::Fill)
        .width_request(84)
        .css_classes(["outpost-slide-edge"])
        .build()
}

fn slide(store: &Rc<Store>, app: &Rc<App>, pick: Pick) -> gtk::Button {
    let badge = gtk::Label::builder()
        .label(match pick {
            Pick::Featured => "Featured",
            Pick::Popular => "Popular",
            Pick::New => "New & Updated",
        })
        .halign(gtk::Align::Center)
        .css_classes(["outpost-badge"])
        .build();
    let icon = gtk::Image::builder().icon_name(PLACEHOLDER).pixel_size(128).build();
    let name = gtk::Label::builder()
        .label(app.name.as_str())
        .wrap(true)
        .justify(gtk::Justification::Center)
        .css_classes(["title-1"])
        .build();
    let summary = gtk::Label::builder()
        .label(app.summary.as_str())
        .wrap(true)
        .justify(gtk::Justification::Center)
        .max_width_chars(60)
        .lines(2)
        .ellipsize(gtk::pango::EllipsizeMode::End)
        .css_classes(["outpost-summary"])
        .build();

    let content = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(12)
        .halign(gtk::Align::Center)
        .valign(gtk::Align::Center)
        .build();
    content.append(&badge);
    content.append(&icon);
    content.append(&name);
    content.append(&summary);

    let button = gtk::Button::builder()
        .child(&content)
        .hexpand(true)
        .height_request(340)
        .css_classes(["outpost-slide"])
        .build();
    if let Some(url) = &app.icon_url {
        let (icon, button) = (icon.downgrade(), button.downgrade());
        icons::with_icon(url, &store.paths().cache, move |texture| {
            if let Some(icon) = icon.upgrade() {
                icon.set_paintable(Some(texture));
            }
            if let Some(button) = button.upgrade() {
                button.add_css_class(&color_class(texture));
            }
        });
    }
    let (store, app) = (store.clone(), app.clone());
    button.connect_clicked(move |_| store.show_app(&app));
    button
}

fn add_css() {
    if CSS_ADDED.replace(true) {
        return;
    }
    let Some(display) = gdk::Display::default() else { return };
    let provider = gtk::CssProvider::new();
    provider.load_from_string(CSS);
    gtk::style_context_add_provider_for_display(&display, &provider, gtk::STYLE_PROVIDER_PRIORITY_APPLICATION);
    COLOR_CSS.with(|colors| {
        gtk::style_context_add_provider_for_display(&display, colors, gtk::STYLE_PROVIDER_PRIORITY_APPLICATION)
    });
}

/// A CSS class that gives a slide the icon's color, worked out once per icon.
fn color_class(texture: &gdk::Texture) -> String {
    let mut hasher = DefaultHasher::new();
    texture.as_ptr().hash(&mut hasher);
    let class = format!("outpost-slide-{:x}", hasher.finish());
    if COLORS.with_borrow(|colors| colors.contains_key(&class)) {
        return class;
    }

    let mut downloader = gdk::TextureDownloader::new(texture);
    downloader.set_format(gdk::MemoryFormat::R8g8b8a8);
    let (pixels, stride) = downloader.download_bytes();
    let color = main_color(&pixels, texture.width() as usize, texture.height() as usize, stride)
        .map_or_else(|| NEUTRAL.to_string(), background);
    COLORS.with_borrow_mut(|colors| {
        colors.insert(class.clone(), format!(".outpost-slide.{class} {{ background-color: {color}; }}"));
        let css: String = colors.values().map(String::as_str).collect::<Vec<_>>().join("\n");
        COLOR_CSS.with(|provider| provider.load_from_string(&css));
    });
    class
}

/// The icon's main color, from tightly packed RGBA rows: the most common vivid
/// hue, ignoring see-through, grey, very dark and very pale pixels. `None` for an
/// icon with hardly any color.
fn main_color(pixels: &[u8], width: usize, height: usize, stride: usize) -> Option<[f64; 3]> {
    // Twelve hue slices, each adding up color weighted by how vivid it is.
    let mut slices = [([0.0f64; 3], 0.0f64); 12];
    let mut opaque = 0.0;
    for y in 0..height {
        for px in pixels[y * stride..].as_chunks::<4>().0.iter().take(width) {
            if px[3] < 200 {
                continue;
            }
            opaque += 1.0;
            let rgb = [px[0], px[1], px[2]].map(|c| c as f64 / 255.0);
            let (hue, saturation, value) = hsv(rgb);
            if saturation < 0.25 || value < 0.25 {
                continue;
            }
            let weight = saturation * value;
            let slice = &mut slices[(hue / 30.0) as usize % 12];
            for (sum, c) in slice.0.iter_mut().zip(rgb) {
                *sum += c * weight;
            }
            slice.1 += weight;
        }
    }
    let (sum, weight) = slices.into_iter().max_by(|a, b| a.1.total_cmp(&b.1))?;
    (weight > 0.04 * opaque && weight > 0.0).then(|| sum.map(|c| c / weight))
}

/// A deep shade of `rgb` that white text reads well on.
fn background(rgb: [f64; 3]) -> String {
    let (hue, saturation, _) = hsl(rgb);
    let [r, g, b] = from_hsl(hue, saturation.clamp(0.3, 0.65), 0.24);
    format!("rgb({}, {}, {})", (r * 255.0).round(), (g * 255.0).round(), (b * 255.0).round())
}

fn hsv([r, g, b]: [f64; 3]) -> (f64, f64, f64) {
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let saturation = if max > 0.0 { (max - min) / max } else { 0.0 };
    (hue([r, g, b], max, min), saturation, max)
}

fn hsl([r, g, b]: [f64; 3]) -> (f64, f64, f64) {
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let lightness = (max + min) / 2.0;
    let saturation = if max == min { 0.0 } else { (max - min) / (1.0 - (2.0 * lightness - 1.0).abs()) };
    (hue([r, g, b], max, min), saturation, lightness)
}

/// In degrees, 0 to 360.
fn hue([r, g, b]: [f64; 3], max: f64, min: f64) -> f64 {
    let range = max - min;
    if range == 0.0 {
        return 0.0;
    }
    let hue = if max == r {
        (g - b) / range
    } else if max == g {
        (b - r) / range + 2.0
    } else {
        (r - g) / range + 4.0
    };
    (hue * 60.0).rem_euclid(360.0)
}

fn from_hsl(hue: f64, saturation: f64, lightness: f64) -> [f64; 3] {
    let chroma = (1.0 - (2.0 * lightness - 1.0).abs()) * saturation;
    let x = chroma * (1.0 - ((hue / 60.0) % 2.0 - 1.0).abs());
    let [r, g, b] = match (hue / 60.0) as u32 {
        0 => [chroma, x, 0.0],
        1 => [x, chroma, 0.0],
        2 => [0.0, chroma, x],
        3 => [0.0, x, chroma],
        4 => [x, 0.0, chroma],
        _ => [chroma, 0.0, x],
    };
    let m = lightness - chroma / 2.0;
    [r + m, g + m, b + m]
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 10×10 RGBA image: `colored` pixels of `color`, the rest white.
    fn image(color: [u8; 4], colored: usize) -> Vec<u8> {
        (0..100).flat_map(|i| if i < colored { color } else { [255, 255, 255, 255] }).collect()
    }

    #[test]
    fn finds_the_main_color_of_a_mostly_white_icon() {
        // The Graphs icon: a white page with a blue line on it.
        let [r, g, b] = main_color(&image([80, 150, 230, 255], 20), 10, 10, 40).unwrap();
        assert!(b > r && b > g, "{r} {g} {b}");
    }

    #[test]
    fn grey_or_clear_icons_have_no_main_color() {
        assert!(main_color(&image([128, 128, 128, 255], 100), 10, 10, 40).is_none());
        assert!(main_color(&image([255, 0, 0, 0], 100), 10, 10, 40).is_none());
    }

    #[test]
    fn backgrounds_are_deep_enough_for_white_text() {
        // Even a pale yellow becomes a dark shade.
        let css = background([1.0, 0.95, 0.4]);
        let channels: Vec<f64> = css
            .trim_start_matches("rgb(")
            .trim_end_matches(')')
            .split(", ")
            .map(|c| c.parse().unwrap())
            .collect();
        assert!(channels.iter().all(|&c| c <= 160.0), "{css}");
        assert!(channels[0] > channels[2], "keeps its hue: {css}");
    }
}
