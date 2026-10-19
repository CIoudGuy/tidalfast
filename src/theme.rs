//! Shared palette, typography, icons, and base widgets.
//!
//! Hanken Grotesk provides real font weights, and Lucide provides a consistent icon set.
//! All colors use [`Palette`] so light, dark, and album-art-tinted themes stay
//! consistent.

use egui::{Color32, CornerRadius, Response, Sense, Stroke, Vec2};

#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Palette {
    pub dark: bool,
    pub window: Color32,
    pub panel: Color32,
    pub surface: Color32,
    pub surface_hover: Color32,
    pub surface_active: Color32,
    pub outline: Color32,
    pub text: Color32,
    pub secondary: Color32,
    pub dim: Color32,
    pub accent: Color32,
    pub accent_hover: Color32,
    pub on_accent: Color32,
    pub danger: Color32,
    pub warning: Color32,
    pub overlay: Color32,
    pub shadow: Color32,
    /// A Luna theme set these colours: the fixed TIDAL Dark shades below
    /// (menus, chips, hairlines) give way to ones made from the palette.
    pub themed: bool,
    /// Corner radius of egui's own windows and menus (TIDAL's
    /// `--wave-border-radius--small`), which themes may change.
    pub radius: u8,
}

impl Palette {
    /// TIDAL's own Dark palette (no theme): the fixed shades sampled from
    /// TIDAL apply. A themed dark palette derives them from its colours.
    pub fn tidal_dark(&self) -> bool {
        self.dark && !self.themed
    }

    /// This palette `t` of the way to `other`, colour by colour; past the
    /// half way the flags and radius are `other`'s too.
    pub fn toward(&self, other: &Palette, t: f32) -> Palette {
        let mix = |a: Color32, b: Color32| crate::motion::blend(a, b, t);
        let near = if t >= 0.5 { other } else { self };
        Palette {
            dark: near.dark,
            window: mix(self.window, other.window),
            panel: mix(self.panel, other.panel),
            surface: mix(self.surface, other.surface),
            surface_hover: mix(self.surface_hover, other.surface_hover),
            surface_active: mix(self.surface_active, other.surface_active),
            outline: mix(self.outline, other.outline),
            text: mix(self.text, other.text),
            secondary: mix(self.secondary, other.secondary),
            dim: mix(self.dim, other.dim),
            accent: mix(self.accent, other.accent),
            accent_hover: mix(self.accent_hover, other.accent_hover),
            on_accent: mix(self.on_accent, other.on_accent),
            danger: mix(self.danger, other.danger),
            warning: mix(self.warning, other.warning),
            overlay: mix(self.overlay, other.overlay),
            shadow: mix(self.shadow, other.shadow),
            themed: near.themed,
            radius: near.radius,
        }
    }

    /// The fill of a primary button: TIDAL's white pill (near-black on the
    /// light theme), a little dimmer under the pointer.
    pub fn primary_fill(&self, hovered: bool) -> Color32 {
        if hovered {
            self.text.lerp_to_gamma(self.window, 0.14)
        } else {
            self.text
        }
    }

    /// The label colour on [`Palette::primary_fill`].
    pub fn on_primary(&self) -> Color32 {
        self.window
    }

    /// TIDAL's frosted chip, rgba(40,40,40,.75): the back/forward pill, the
    /// search field and header chips.
    pub fn chip(&self) -> Color32 {
        if self.tidal_dark() {
            Color32::from_rgba_unmultiplied(40, 40, 40, 191)
        } else {
            self.surface
        }
    }

    /// A navigation row under the pointer (TIDAL surface-10, #141414).
    pub fn nav_hover(&self) -> Color32 {
        if self.tidal_dark() {
            Color32::from_rgb(0x14, 0x14, 0x14)
        } else {
            self.surface
        }
    }

    /// The selected navigation row (white at 15 %).
    pub fn nav_selected(&self) -> Color32 {
        if self.tidal_dark() {
            Color32::from_white_alpha(38)
        } else {
            self.surface_active
        }
    }

    /// TIDAL text-30: section labels and quiet icons (white at 30 %).
    pub fn tertiary(&self) -> Color32 {
        if self.tidal_dark() {
            Color32::from_rgb(0x4d, 0x4d, 0x4d)
        } else {
            self.dim
        }
    }

    /// TIDAL's 1-point dividers (white at 8 %).
    pub fn hairline(&self) -> Color32 {
        if self.tidal_dark() {
            Color32::from_white_alpha(20)
        } else {
            self.outline
        }
    }

    /// The one colour of every menu surface: context menus and their
    /// submenus, the avatar and "..." menus, the quality and output popovers,
    /// the collection flyout, select popups and tooltips (TIDAL surface-30,
    /// #2d2d2d, sampled from its context, avatar and collection menus).
    pub fn menu_bg(&self) -> Color32 {
        if self.tidal_dark() {
            Color32::from_rgb(0x2d, 0x2d, 0x2d)
        } else {
            self.overlay
        }
    }

    /// A menu row under the pointer: white at 15 % in Dark.
    pub fn menu_hover(&self) -> Color32 {
        if self.tidal_dark() {
            Color32::from_white_alpha(38)
        } else {
            self.surface_hover
        }
    }

    /// The line between menu groups (white at 8 %).
    pub fn menu_separator(&self) -> Color32 {
        self.hairline()
    }

    /// A menu's border: TIDAL's Dark menus have none.
    pub fn menu_border(&self) -> Stroke {
        if self.tidal_dark() {
            Stroke::NONE
        } else {
            Stroke::new(1.0, self.outline)
        }
    }

    /// Secondary text and chevrons inside a menu.
    pub fn menu_secondary(&self) -> Color32 {
        if self.tidal_dark() {
            Color32::from_rgb(0xaf, 0xaf, 0xb6)
        } else {
            self.secondary
        }
    }

    /// The search dropdown is TIDAL's one popup that is not a menu: it sits
    /// on the surface colour (#18181b), as the reference shows.
    pub fn search_popup_bg(&self) -> Color32 {
        if self.tidal_dark() {
            self.surface
        } else {
            self.overlay
        }
    }

    /// TIDAL Dark: true black, cool near-black surfaces and the cyan accent.
    pub fn dark() -> Self {
        Self {
            dark: true,
            window: Color32::from_rgb(0x00, 0x00, 0x00),
            panel: Color32::from_rgb(0x0a, 0x0a, 0x0c),
            surface: Color32::from_rgb(0x18, 0x18, 0x1b),
            surface_hover: Color32::from_rgb(0x20, 0x20, 0x24),
            surface_active: Color32::from_rgb(0x2a, 0x2a, 0x2e),
            outline: Color32::from_rgb(0x26, 0x26, 0x2b),
            text: Color32::from_rgb(0xff, 0xff, 0xff),
            // TIDAL text-20: white at 55 % over black.
            secondary: Color32::from_rgb(0x8c, 0x8c, 0x8c),
            dim: Color32::from_rgb(0x79, 0x77, 0x7f),
            accent: Color32::from_rgb(0x33, 0xff, 0xee),
            accent_hover: Color32::from_rgb(0x7d, 0xff, 0xf4),
            on_accent: Color32::from_rgb(0x00, 0x00, 0x00),
            danger: Color32::from_rgb(0xff, 0x6b, 0x7a),
            warning: Color32::from_rgb(0xff, 0xd4, 0x32),
            overlay: Color32::from_rgb(0x18, 0x18, 0x1b),
            shadow: Color32::from_black_alpha(160),
            themed: false,
            radius: RADIUS,
        }
    }

    /// TIDAL Light: near-white surfaces with a deeper teal accent that keeps
    /// 4.5:1 against white text and 3:1 against the window.
    pub fn light() -> Self {
        Self {
            dark: false,
            window: Color32::from_rgb(0xf5, 0xf5, 0xf7),
            panel: Color32::from_rgb(0xff, 0xff, 0xff),
            surface: Color32::from_rgb(0xed, 0xed, 0xf0),
            surface_hover: Color32::from_rgb(0xe2, 0xe2, 0xe6),
            surface_active: Color32::from_rgb(0xd5, 0xd5, 0xda),
            outline: Color32::from_rgb(0xdb, 0xdb, 0xe0),
            text: Color32::from_rgb(0x0a, 0x0a, 0x0b),
            secondary: Color32::from_rgb(0x55, 0x55, 0x5c),
            dim: Color32::from_rgb(0x6e, 0x6e, 0x76),
            accent: Color32::from_rgb(0x00, 0x78, 0x6e),
            accent_hover: Color32::from_rgb(0x00, 0x62, 0x59),
            on_accent: Color32::WHITE,
            danger: Color32::from_rgb(0xd0, 0x2f, 0x45),
            warning: Color32::from_rgb(0x8a, 0x66, 0x00),
            overlay: Color32::from_rgb(0xff, 0xff, 0xff),
            shadow: Color32::from_black_alpha(50),
            themed: false,
            radius: RADIUS,
        }
    }

    /// A colour derived from album art, softened so it can sit behind text.
    pub fn tint_from_art(&self, rgb: [u8; 3]) -> Color32 {
        let [r, g, b] = rgb.map(|c| c as f32 / 255.0);
        let max = r.max(g).max(b);
        let min = r.min(g).min(b);
        let lightness = (max + min) / 2.0;
        let target = if self.dark { 0.30 } else { 0.72 };
        let (r, g, b) = if lightness < 0.01 {
            (target, target, target)
        } else {
            let scale = target / lightness;
            (
                (r * scale).min(1.0),
                (g * scale).min(1.0),
                (b * scale).min(1.0),
            )
        };
        Color32::from_rgb((r * 255.0) as u8, (g * 255.0) as u8, (b * 255.0) as u8)
    }
}

pub const RADIUS: u8 = 8;
pub const RADIUS_SMALL: u8 = 4;
pub const ROW_HEIGHT: f32 = 56.0;
pub const COMPACT_ROW_HEIGHT: f32 = 48.0;
pub const PLAYER_BAR_HEIGHT: f32 = 88.0;
/// The narrowest either right-hand panel goes. The queue and the lyrics
/// take the same edge and swap places there, so a width that suits one
/// has to suit the other, or the window would jump on the swap.
pub const SIDE_PANEL_MIN_WIDTH: f32 = 280.0;
pub const TOP_BAR_HEIGHT: f32 = 56.0;

/// TIDAL's 56-point header. On Windows the caption buttons sit in the title
/// bar above it, so it keeps its height there too.
pub fn top_bar_height() -> f32 {
    TOP_BAR_HEIGHT
}

/// macOS hides the titlebar and draws the window content all the way to the
/// top edge, so whatever sits at the top of the window has to leave room for
/// the traffic lights. Zero everywhere else, and in fullscreen, where the
/// buttons are gone.
pub fn titlebar_inset(ctx: &egui::Context) -> f32 {
    if cfg!(target_os = "macos") && !ctx.input(|input| input.viewport().fullscreen.unwrap_or(false))
    {
        28.0
    } else {
        0.0
    }
}

pub fn regular(size: f32) -> egui::FontId {
    fastframe_fonts::Weight::Regular.font_id(size)
}

pub fn medium(size: f32) -> egui::FontId {
    fastframe_fonts::Weight::Medium.font_id(size)
}

pub fn semibold(size: f32) -> egui::FontId {
    fastframe_fonts::Weight::SemiBold.font_id(size)
}

pub fn bold(size: f32) -> egui::FontId {
    fastframe_fonts::Weight::Bold.font_id(size)
}

/// How the desktop renders text, read once per process.
///
/// Tests use the platform's default instead of asking the desktop, so they
/// neither wait on D-Bus nor depend on the machine's settings.
pub fn text_rendering() -> fastframe_text::TextRendering {
    static RENDERING: std::sync::OnceLock<fastframe_text::TextRendering> =
        std::sync::OnceLock::new();
    *RENDERING.get_or_init(|| {
        if cfg!(test) {
            fastframe_text::TextRendering::platform_default()
        } else {
            fastframe_text::detect()
        }
    })
}

/// Install fonts, icons, and the base style once.
pub fn install(ctx: &egui::Context) {
    install_fonts(ctx);
    egui_extras::install_image_loaders(ctx);
    fastframe_icons::install::<Icon>(ctx);
    // Colour emoji over every text egui draws (see `crate::emoji`).
    ctx.add_plugin(fastframe_emoji::EmojiPlugin::default());
}

/// Applies the palette to egui's own widgets so dialogs, menus, and text
/// fields agree with the custom views.
pub fn apply(ctx: &egui::Context, palette: &Palette) {
    let mut style = (*ctx.global_style()).clone();
    apply_to_style(&mut style, palette);
    ctx.set_global_style(style);
}

/// Applies a palette to this view and children without changing global style.
pub fn apply_local(ui: &mut egui::Ui, palette: &Palette) {
    apply_to_style(ui.style_mut(), palette);
}

fn apply_to_style(style: &mut egui::Style, palette: &Palette) {
    let visuals = &mut style.visuals;
    *visuals = if palette.dark {
        egui::Visuals::dark()
    } else {
        egui::Visuals::light()
    };
    visuals.dark_mode = palette.dark;
    // Glyph coverage, hinting and sub-pixel positions as the desktop draws
    // them: linear coverage in both themes on Linux, where egui's dark curve
    // (2c - c²) made text heavier than GTK's.
    text_rendering().apply_to_visuals(visuals);
    visuals.panel_fill = palette.panel;
    // egui draws submenus, select popups and tooltips from `window_fill`;
    // dialogs and toasts paint `overlay` themselves, so this is the menu colour.
    visuals.window_fill = palette.menu_bg();
    visuals.extreme_bg_color = palette.surface;
    visuals.faint_bg_color = palette.surface;
    visuals.code_bg_color = palette.surface;
    visuals.override_text_color = Some(palette.text);
    visuals.weak_text_color = Some(palette.secondary);
    visuals.hyperlink_color = palette.text;
    visuals.selection.bg_fill = palette.accent.gamma_multiply(0.35);
    visuals.selection.stroke = Stroke::new(1.0, palette.accent);
    visuals.window_stroke = Stroke::new(1.0, palette.outline);
    visuals.window_corner_radius = CornerRadius::same(palette.radius.saturating_add(2));
    visuals.menu_corner_radius = CornerRadius::same(palette.radius);
    visuals.window_shadow = egui::epaint::Shadow {
        offset: [0, 6],
        blur: 24,
        spread: 0,
        color: palette.shadow,
    };
    visuals.popup_shadow = egui::epaint::Shadow {
        offset: [0, 4],
        blur: 16,
        spread: 0,
        color: palette.shadow,
    };
    let corner = CornerRadius::same((palette.radius / 2).saturating_add(2));
    for widget in [
        &mut visuals.widgets.inactive,
        &mut visuals.widgets.hovered,
        &mut visuals.widgets.active,
        &mut visuals.widgets.open,
    ] {
        widget.corner_radius = corner;
        widget.bg_stroke = Stroke::NONE;
        widget.fg_stroke = Stroke::new(1.0, palette.text);
        widget.expansion = 0.0;
    }
    visuals.widgets.noninteractive.corner_radius = corner;
    visuals.widgets.noninteractive.bg_fill = palette.panel;
    visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0, palette.outline);
    visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0, palette.text);
    visuals.widgets.inactive.bg_fill = palette.surface;
    visuals.widgets.inactive.weak_bg_fill = palette.surface;
    visuals.widgets.hovered.bg_fill = palette.surface_hover;
    visuals.widgets.hovered.weak_bg_fill = palette.surface_hover;
    visuals.widgets.active.bg_fill = palette.surface_active;
    visuals.widgets.active.weak_bg_fill = palette.surface_active;
    visuals.widgets.open.bg_fill = palette.surface_hover;
    visuals.widgets.open.weak_bg_fill = palette.surface_hover;
    visuals.text_cursor.stroke = Stroke::new(2.0, palette.accent);
    visuals.striped = false;
    visuals.slider_trailing_fill = true;
    visuals.handle_shape = egui::style::HandleShape::Circle;

    use egui::FontFamily::{Monospace, Proportional};
    use egui::{FontId, TextStyle};
    style.text_styles = [
        (TextStyle::Small, FontId::new(11.5, Proportional)),
        (TextStyle::Body, FontId::new(14.0, Proportional)),
        (TextStyle::Button, FontId::new(14.0, Proportional)),
        (TextStyle::Heading, FontId::new(22.0, Proportional)),
        (TextStyle::Monospace, FontId::new(13.0, Monospace)),
    ]
    .into();
    style.spacing.item_spacing = Vec2::new(8.0, 6.0);
    style.spacing.button_padding = Vec2::new(12.0, 6.0);
    style.spacing.interact_size = Vec2::new(40.0, 28.0);
    style.spacing.menu_margin = egui::Margin::same(6);
    style.spacing.window_margin = egui::Margin::same(16);
    style.spacing.scroll = egui::style::ScrollStyle {
        bar_width: 8.0,
        floating_width: 6.0,
        floating_allocated_width: 0.0,
        handle_min_length: 28.0,
        bar_inner_margin: 3.0,
        bar_outer_margin: 2.0,
        dormant_background_opacity: 0.0,
        dormant_handle_opacity: 0.0,
        active_background_opacity: 0.0,
        active_handle_opacity: 0.55,
        interact_handle_opacity: 0.85,
        foreground_color: true,
        ..egui::style::ScrollStyle::floating()
    };
    style.interaction.selectable_labels = false;
    style.interaction.tooltip_delay = 0.4;
    style.animation_time = crate::motion::ANIMATION_TIME;
    style.url_in_tooltip = false;
}

/// Hanken Grotesk at its four weights with the monochrome emoji face right behind it
/// (so every emoji wears the same style, ahead of egui's own pair), then the
/// installed faces for the scripts Hanken lacks, drawn the way the desktop
/// renders text.
fn install_fonts(ctx: &egui::Context) {
    install_plugin_fonts(ctx, &[], None);
}

/// The app's fonts plus fonts Luna plugins loaded (custom-fonts), each as
/// its own named family falling back to Hanken; `primary` also leads the
/// proportional family. Empty `extra` restores the app's own fonts.
pub fn install_plugin_fonts(
    ctx: &egui::Context,
    extra: &[(String, [std::sync::Arc<egui::FontData>; 4])],
    primary: Option<&str>,
) {
    let emoji = egui::FontData::from_static(include_bytes!("../assets/fonts/NotoEmoji.ttf"));
    let mut fonts = fastframe_fonts::FontSetup::default()
        .companion("noto_emoji", std::sync::Arc::new(emoji))
        .definitions();
    let proportional = fonts
        .families
        .get(&egui::FontFamily::Proportional)
        .cloned()
        .unwrap_or_default();
    for (name, faces) in extra {
        // egui panics on a font it cannot parse, on the UI thread: a broken
        // one from a theme or plugin is left out, never installed.
        if !faces
            .iter()
            .all(|data| crate::ui::plugin_dom::font_parses(&data.font, data.index))
        {
            continue;
        }
        let keys = fastframe_fonts::Weight::ALL.map(|weight| {
            let key = format!("luna:{name}:{}", weight.name());
            fonts
                .font_data
                .insert(key.clone(), faces[weight_slot(weight)].clone());
            key
        });
        let mut family = vec![keys[0].clone()];
        family.extend(proportional.iter().cloned());
        fonts
            .families
            .insert(egui::FontFamily::Name(name.as_str().into()), family);
        if primary == Some(name.as_str()) {
            lead_with(&mut fonts, &keys);
        }
    }
    use_hanken(&mut fonts);
    crate::fonts::apply(&mut fonts);
    text_rendering().apply_to(&mut fonts);
    ctx.set_fonts(fonts);
}

/// Where a weight sits in the four faces of a plugin font.
const fn weight_slot(weight: fastframe_fonts::Weight) -> usize {
    match weight {
        fastframe_fonts::Weight::Regular => 0,
        fastframe_fonts::Weight::Medium => 1,
        fastframe_fonts::Weight::SemiBold => 2,
        fastframe_fonts::Weight::Bold => 3,
    }
}

/// Puts the page font first in every weight's family, each weight in its own
/// face of it (`keys`, in the order of [`weight_slot`]): the page's font is
/// every weight's, so a theme that sets the font of everything
/// (`* { font-family: ... }`) reaches titles and bold text as well, bold in
/// the font's bold (egui does not thicken a regular face).
fn lead_with(fonts: &mut egui::FontDefinitions, keys: &[String; 4]) {
    for weight in fastframe_fonts::Weight::ALL {
        let key = &keys[weight_slot(weight)];
        if let Some(family) = fonts.families.get_mut(&weight.family())
            && family.first() != Some(key)
        {
            family.insert(0, key.clone());
        }
    }
}

/// Hanken Grotesk (variable, OFL), the interface face. The bundled file is
/// the Google Fonts release with its Greek, Greek Extended and Cyrillic
/// character-map entries removed (µ, π, Δ, Ω and the tonos marks were the
/// only ones; see assets/fonts/README-fonts.txt), so a Greek or Cyrillic
/// run is drawn by one face, Inter, never half by Hanken (src/fonts.rs).
pub const HANKEN: &[u8] = include_bytes!("../assets/fonts/HankenGrotesk.ttf");

/// The Hanken `wght` that stands for a weight: TIDAL's numbered weights run
/// heavy, so each step maps one step up (regular 500 ... bold 800).
pub const fn hanken_weight(weight: fastframe_fonts::Weight) -> f32 {
    match weight {
        fastframe_fonts::Weight::Regular => 500.0,
        fastframe_fonts::Weight::Medium => 600.0,
        fastframe_fonts::Weight::SemiBold => 700.0,
        fastframe_fonts::Weight::Bold => 800.0,
    }
}

/// Swaps the four weight faces fastframe registers for Hanken Grotesk, keeping
/// every key and family name. TIDAL's numbered weights run heavy, so each
/// step maps one step up: regular 500, medium 600, semibold 700, bold 800.
/// Scale 1.03 makes up for Hanken's smaller x-height. The fallback faces
/// (scripts, emoji) behind it stay as they were. Hanken's digits are all the
/// same width, so durations and counters need no tabular feature.
fn use_hanken(fonts: &mut egui::FontDefinitions) {
    for weight in fastframe_fonts::Weight::ALL {
        let wght = hanken_weight(weight);
        let mut data = egui::FontData::from_static(HANKEN);
        data.tweak.coords = egui::epaint::text::VariationCoords::new([(b"wght", wght)]);
        data.tweak.scale = 1.03;
        fonts
            .font_data
            .insert(weight.name().to_owned(), std::sync::Arc::new(data));
    }
}

fastframe_icons::icons! {
    /// Every icon the interface draws. The shared Lucide icons come from
    /// fastframe-icons; the rest are Tidalfast's own files.
    pub enum Icon {
        prefix: "tidalfast-icon-",
        directory: "../assets/icons/",
        ArrowLeft => lucide "arrow-left",
        ArrowLeftToLine => "arrow-left-to-line",
        ArrowRight => "arrow-right",
        ArrowRightToLine => "arrow-right-to-line",
        ArrowUp => "arrow-up",
        ArrowUpDown => "arrow-up-down",
        AudioLines => "audio-lines",
        Ban => "ban",
        BadgeCheck => "badge-check",
        Bell => "bell",
        Bookmark => "bookmark",
        BookmarkFilled => "bookmark-filled",
        Calendar => "calendar",
        Car => "car",
        Cast => "cast",
        Check => lucide "check",
        ChevronDown => lucide "chevron-down",
        ChevronLeft => lucide "chevron-left",
        ChevronRight => lucide "chevron-right",
        ChevronUp => lucide "chevron-up",
        CircleAlert => lucide "circle-alert",
        CircleCheck => lucide "circle-check",
        CirclePlay => "circle-play",
        CirclePlus => "circle-plus",
        CirclePlusFilled => "circle-plus-filled",
        CircleX => lucide "circle-x",
        Clock => lucide "clock",
        Code => "code",
        Compass => "compass",
        Copy => lucide "copy",
        Disc => "disc-3",
        Download => "arrow-down-to-line",
        Ellipsis => lucide "ellipsis",
        Expand => "expand",
        ExternalLink => lucide "external-link",
        FolderPlus => "folder-plus",
        Gamepad => "gamepad-2",
        Globe => "globe",
        GripVertical => "grip-vertical",
        Headphones => "headphones",
        Heart => "heart",
        HeartFilled => "heart-filled",
        House => "house",
        Info => lucide "info",
        LockOpen => "lock-open",
        Laptop => "laptop",
        Library => "library",
        LayoutGrid => "layout-grid",
        Link => "link",
        LayoutList => "layout-list",
        ListEnd => "list-end",
        ListMusic => "list-music",
        ListPlus => "list-plus",
        ListVideo => "list-video",
        Loader => "loader-circle",
        LyricsSync => "lyrics-sync",
        Lock => lucide "lock",
        LogOut => lucide "log-out",
        MessageSquareQuote => "message-square-quote",
        Mic => lucide "mic",
        Minus => lucide "minus",
        Monitor => lucide "monitor",
        Moon => lucide "moon",
        Music => "music",
        NotePlus => "note-plus",
        Pause => lucide "pause",
        PauseFilled => "pause-filled",
        PanelLeft => lucide "panel-left",
        Pin => lucide "pin",
        PinOff => lucide "pin-off",
        Pencil => lucide "pencil",
        PictureInPicture => "picture-in-picture",
        Play => lucide "play",
        PlayFilled => "play-filled",
        Plus => lucide "plus",
        Radio => "radio",
        Refresh => lucide "refresh-cw",
        Repeat => "repeat",
        Repeat1 => "repeat-1",
        Search => lucide "search",
        Settings => lucide "settings",
        Share => "share",
        ShareNodes => "share-nodes",
        Shrink => "shrink",
        Shuffle => "shuffle",
        SkipBack => "skip-back",
        SkipBackFilled => "skip-back-filled",
        SkipForward => "skip-forward",
        SkipForwardFilled => "skip-forward-filled",
        Smartphone => lucide "smartphone",
        Sparkles => "sparkles",
        Speaker => "speaker",
        Square => "square",
        SquareE => "square-e",
        SquarePlay => "square-play",
        Star => "star",
        SquarePen => lucide "square-pen",
        Sun => lucide "sun",
        Tablet => "tablet",
        Tag => "tag",
        Trash => lucide "trash-2",
        TrendingUp => "trending-up",
        Upload => "arrow-up-from-line",
        Trophy => "trophy",
        Tv => "tv",
        User => lucide "user",
        Users => lucide "users",
        Volume => "volume",
        Videos => "videos",
        Volume1 => "volume-1",
        Volume2 => lucide "volume-2",
        VolumeX => lucide "volume-x",
        Watch => "watch",
        WifiOff => "wifi-off",
        X => lucide "x",
        Zap => "zap",
    }
}

/// A static icon.
pub fn icon(ui: &mut egui::Ui, icon: Icon, size: f32, color: Color32) -> Response {
    ui.add(icon.image(color, size))
}

/// Paints an icon centred in `rect` without allocating space.
pub fn paint_icon(ui: &egui::Ui, icon: Icon, rect: egui::Rect, size: f32, color: Color32) {
    let icon_rect = egui::Rect::from_center_size(
        rect.center() + play_glyph_offset(icon, size),
        Vec2::splat(size),
    );
    icon.image(color, size).paint_at(ui, icon_rect);
}

/// Make keyboard focus visible without changing the control's layout.
pub fn focus_ring(ui: &egui::Ui, response: &Response) {
    if response.has_focus() {
        ui.painter().rect_stroke(
            response.rect.expand(2.0),
            4.0,
            ui.visuals().selection.stroke,
            egui::StrokeKind::Outside,
        );
    }
    if response.gained_focus() {
        response.scroll_to_me(None);
    }
}

/// The padding [`icon_button`] keeps around its glyph on every side. A
/// caller that places an icon button inside a fixed box must give the box
/// at least `size + ICON_BUTTON_PAD`: in a tighter one, egui clamps the
/// centered layout to the box's top edge and the glyph lands off-center.
pub const ICON_BUTTON_PAD: f32 = 12.0;

/// A frameless icon control whose colour lifts on hover.
pub fn icon_button(
    ui: &mut egui::Ui,
    icon: Icon,
    size: f32,
    color: Color32,
    hover: Color32,
    tooltip: &str,
) -> Response {
    icon_button_scaled(ui, icon, size, color, hover, tooltip, 1.0)
}

/// An [`icon_button`] of the player bar, which TIDAL grows to 1.1 under the
/// pointer (`._player button:hover { transform: scale(1.1) }`).
pub fn player_icon_button(
    ui: &mut egui::Ui,
    icon: Icon,
    size: f32,
    color: Color32,
    hover: Color32,
    tooltip: &str,
) -> Response {
    icon_button_scaled(
        ui,
        icon,
        size,
        color,
        hover,
        tooltip,
        crate::motion::tidal::HOVER_SCALE,
    )
}

fn icon_button_scaled(
    ui: &mut egui::Ui,
    icon: Icon,
    size: f32,
    color: Color32,
    hover: Color32,
    tooltip: &str,
    hover_scale: f32,
) -> Response {
    let edge = size + ICON_BUTTON_PAD;
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(edge), Sense::click());
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), tooltip)
    });
    if ui.is_rect_visible(rect) {
        // TIDAL: `color .15s`, and `scale(.9)` while pressed (`transform .15s`).
        let tint = crate::motion::color(
            ui.ctx(),
            response.id.with("tint"),
            response.hovered() || response.has_focus(),
            color,
            hover,
            crate::motion::tidal::ICON,
        );
        let scale = crate::motion::press_scale(
            ui.ctx(),
            &response,
            if ui.is_enabled() { hover_scale } else { 1.0 },
            crate::motion::tidal::PRESS_SCALE,
        );
        paint_icon(ui, icon, rect, size * scale, tint);
    }
    focus_ring(ui, &response);
    if tooltip.is_empty() {
        response
    } else {
        response.on_hover_text(tooltip)
    }
}

/// Horizontal offset that optically centers play triangles.
///
/// Lucide includes a 1/24-width shift; a measured 3% shift centers the icon at
/// Tidalfast's sizes. Use this everywhere instead of per-call adjustments.
pub fn play_glyph_offset(icon: Icon, icon_size: f32) -> Vec2 {
    if matches!(icon, Icon::PlayFilled | Icon::Play) {
        Vec2::new(icon_size * (0.03 - 1.0 / 24.0), 0.0)
    } else {
        Vec2::ZERO
    }
}

/// The app's mark as the app draws it: the app icon's glyph without its
/// black square, rasterised once per size by `util::logo_glyph_rgba` and
/// tinted `color` (the text colour, so it follows Dark, Light and themes).
pub fn logo(ui: &egui::Ui, center: egui::Pos2, diameter: f32, color: Color32) {
    let ppp = ui.ctx().pixels_per_point();
    // The raster keeps two pixels of margin on each side of the disc.
    let pixels = (diameter * ppp).round() as usize + 4;
    let id = egui::Id::new(("tidalfast-logo-glyph", pixels));
    let texture = ui
        .ctx()
        .data(|data| data.get_temp::<egui::TextureHandle>(id))
        .unwrap_or_else(|| {
            let image = egui::ColorImage::from_rgba_unmultiplied(
                [pixels, pixels],
                &crate::util::logo_glyph_rgba(pixels),
            );
            let texture =
                ui.ctx()
                    .load_texture("tidalfast-logo-glyph", image, egui::TextureOptions::LINEAR);
            ui.ctx()
                .data_mut(|data| data.insert_temp(id, texture.clone()));
            texture
        });
    let side = pixels as f32 / ppp;
    let rect = egui::Rect::from_center_size(center, Vec2::splat(side));
    ui.painter().image(
        texture.id(),
        rect,
        egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
        color,
    );
}

/// A round button laid over `parent` (a card). It looks and answers like a
/// button but is no widget that takes clicks, so every pointer button that
/// lands on it still reaches `parent`: a right-click on a card's Play button
/// opens the card's menu like one anywhere else on the card. Answers whether
/// it was activated (a primary click on it, or Enter or Space while it holds
/// the focus).
#[expect(clippy::too_many_arguments)]
pub fn circle_button(
    ui: &mut egui::Ui,
    parent: &Response,
    icon: Icon,
    diameter: f32,
    fill: Color32,
    fill_hover: Color32,
    icon_color: Color32,
    tooltip: &str,
) -> bool {
    let (rect, response) =
        ui.allocate_exact_size(Vec2::splat(diameter), Sense::focusable_noninteractive());
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), tooltip)
    });
    let over = ui.rect_contains_pointer(rect);
    let pressed = over && parent.is_pointer_button_down_on();
    let clicked = parent.clicked()
        && parent
            .interact_pointer_pos()
            .is_some_and(|pos| rect.contains(pos));
    let keyed = response.has_focus()
        && ui.input(|input| {
            input.key_pressed(egui::Key::Enter) || input.key_pressed(egui::Key::Space)
        });
    if ui.is_rect_visible(rect) {
        // TIDAL's round play buttons fade their fill (`.2s`) and shrink to .9
        // while pressed.
        let fill = crate::motion::color(
            ui.ctx(),
            response.id.with("fill"),
            over,
            fill,
            fill_hover,
            crate::motion::tidal::HOVER,
        );
        let scale = crate::motion::value(
            ui.ctx(),
            response.id.with("press-scale"),
            if pressed {
                crate::motion::tidal::PRESS_SCALE
            } else {
                1.0
            },
            crate::motion::tidal::PRESS,
        );
        ui.painter()
            .circle_filled(rect.center(), diameter / 2.0 * scale, fill);
        let icon_size = diameter * 0.46 * scale;
        let offset = play_glyph_offset(icon, icon_size);
        let icon_rect =
            egui::Rect::from_center_size(rect.center() + offset, Vec2::splat(icon_size));
        icon.image(icon_color, icon_size).paint_at(ui, icon_rect);
    }
    focus_ring(ui, &response);
    if over && !tooltip.is_empty() {
        response.on_hover_text(tooltip);
    }
    clicked || keyed
}

/// A disc the size of a [`circle_button`] whose icon is replaced by a
/// spinner: the pressed play button itself shows that TIDAL is reacting.
pub fn circle_spinner(
    ui: &mut egui::Ui,
    diameter: f32,
    fill: Color32,
    spin: Color32,
    tooltip: &str,
) -> Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(diameter), Sense::hover());
    response.widget_info(|| {
        egui::WidgetInfo::labeled(
            egui::WidgetType::ProgressIndicator,
            ui.is_enabled(),
            tooltip,
        )
    });
    if ui.is_rect_visible(rect) {
        ui.painter()
            .circle_filled(rect.center(), diameter / 2.0, fill);
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(rect).layout(
            egui::Layout::centered_and_justified(egui::Direction::LeftToRight),
        ));
        spinner(&mut child, diameter * 0.55, spin);
    }
    if tooltip.is_empty() {
        response
    } else {
        response.on_hover_text(tooltip)
    }
}

/// A pill-shaped text button: TIDAL's white pill for the primary action,
/// outlined otherwise.
pub fn pill_button(ui: &mut egui::Ui, palette: &Palette, label: &str, primary: bool) -> Response {
    pill_button_named(ui, palette, label, label, primary)
}

/// A [`pill_button`] whose screen-reader name is `name` (a card's "Install"
/// pill is "Install \<plugin>").
pub fn pill_button_named(
    ui: &mut egui::Ui,
    palette: &Palette,
    label: &str,
    name: &str,
    primary: bool,
) -> Response {
    let font = semibold(13.0);
    let color = if primary {
        palette.on_primary()
    } else {
        palette.text
    };
    let galley = ui.painter().layout_no_wrap(label.to_string(), font, color);
    let padding = Vec2::new(18.0, 8.0);
    let size = galley.size() + padding * 2.0;
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    response
        .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), name));
    if ui.is_rect_visible(rect) {
        let hovered = response.hovered();
        // TIDAL: the fill fades (`background-color .2s linear`) and the pill
        // shrinks to .97 while pressed (`transform .15s`).
        let scale = crate::motion::press_scale(
            ui.ctx(),
            &response,
            1.0,
            crate::motion::tidal::PILL_PRESS_SCALE,
        );
        let rect = egui::Rect::from_center_size(rect.center(), rect.size() * scale);
        let radius = rect.height() / 2.0;
        let fade = crate::motion::progress(
            ui.ctx(),
            response.id.with("fill"),
            hovered,
            crate::motion::Transition::new(0.2, crate::motion::ease::LINEAR),
        );
        if primary {
            ui.painter().rect_filled(
                rect,
                radius,
                crate::motion::blend(
                    palette.primary_fill(false),
                    palette.primary_fill(true),
                    fade,
                ),
            );
        } else {
            let stroke_color = crate::motion::blend(palette.dim, palette.text, fade);
            ui.painter().rect_stroke(
                rect,
                radius,
                Stroke::new(1.0, stroke_color),
                egui::StrokeKind::Inside,
            );
        }
        let pos = rect.center() - galley.size() / 2.0;
        ui.painter().galley(pos, galley, color);
    }
    focus_ring(ui, &response);
    response
}

/// A muted button with an icon and label, for row and header actions.
pub fn soft_button(
    ui: &mut egui::Ui,
    palette: &Palette,
    icon: Option<Icon>,
    label: &str,
    active: bool,
) -> Response {
    soft_button_inner(ui, palette, icon, label, active, false).0
}

/// A [`soft_button`] whose icon turns into a cross on hover, allowing the
/// entry to be dismissed. The returned flag reports clicks on that cross.
pub fn soft_button_dismiss(
    ui: &mut egui::Ui,
    palette: &Palette,
    icon: Icon,
    label: &str,
) -> (Response, bool) {
    soft_button_inner(ui, palette, Some(icon), label, false, true)
}

/// The width `soft_button` gives a button without an icon, for laying out
/// a row of them before drawing it.
pub fn soft_button_width(ui: &egui::Ui, label: &str) -> f32 {
    let galley = crate::bidi::layout_line(ui.painter(), label, medium(13.0), Color32::WHITE);
    galley.size().x + 24.0
}

fn soft_button_inner(
    ui: &mut egui::Ui,
    palette: &Palette,
    icon: Option<Icon>,
    label: &str,
    active: bool,
    dismissible: bool,
) -> (Response, bool) {
    let font = medium(13.0);
    let color = if active { palette.window } else { palette.text };
    let galley = crate::bidi::layout_line(ui.painter(), label, font, color);
    let icon_size = 15.0;
    let icon_width = if icon.is_some() { icon_size + 6.0 } else { 0.0 };
    let padding = Vec2::new(12.0, 7.0);
    let size = Vec2::new(galley.size().x + icon_width, galley.size().y) + padding * 2.0;
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), label)
    });
    let icon_rect = egui::Rect::from_center_size(
        egui::pos2(rect.left() + padding.x + icon_size / 2.0, rect.center().y),
        Vec2::splat(icon_size),
    );
    // Claimed after the button, so the cross sits on top and keeps its own click.
    let dismiss = (dismissible && icon.is_some()).then(|| {
        let dismiss = ui.interact(
            icon_rect.expand(3.0),
            response.id.with("dismiss"),
            Sense::click(),
        );
        dismiss.widget_info(|| {
            egui::WidgetInfo::labeled(
                egui::WidgetType::Button,
                ui.is_enabled(),
                format!("Remove {label}"),
            )
        });
        dismiss
    });
    let dismissed = dismiss.as_ref().is_some_and(Response::clicked);
    let over_dismiss = dismiss
        .as_ref()
        .is_some_and(|dismiss| dismiss.hovered() || dismiss.has_focus());
    if ui.is_rect_visible(rect) {
        let hovered = response.hovered() || over_dismiss;
        let fill = if active {
            palette.text
        } else {
            crate::motion::color(
                ui.ctx(),
                response.id.with("fill"),
                hovered,
                palette.surface,
                palette.surface_hover,
                crate::motion::tidal::HOVER,
            )
        };
        ui.painter().rect_filled(rect, rect.height() / 2.0, fill);
        let mut x = rect.left() + padding.x;
        if let Some(icon) = icon {
            let icon = if dismiss.is_some() && hovered {
                Icon::X
            } else {
                icon
            };
            icon.image(color, icon_size).paint_at(ui, icon_rect);
            x += icon_width;
        }
        let pos = egui::pos2(x, rect.center().y - galley.size().y / 2.0);
        ui.painter().galley(pos, galley, color);
    }
    focus_ring(ui, &response);
    if let Some(dismiss) = dismiss {
        focus_ring(ui, &dismiss);
    }
    (response, dismissed)
}

/// Asks for the next frame `interval` from now.
///
/// egui takes one predicted frame off every delayed repaint, and egui-winit
/// leaves that prediction at 1/60 s whatever the display, so asking egui for
/// 33 ms brought the frame back after 16 ms. The prediction is added back
/// here, for animations that mean to run slower than the display.
#[track_caller]
pub fn repaint_after(ctx: &egui::Context, interval: std::time::Duration) {
    let predicted = ctx.input(|input| input.predicted_dt);
    ctx.request_repaint_after(interval + std::time::Duration::from_secs_f32(predicted));
}

/// How often an endless decoration is drawn (the skeleton shimmer, a busy
/// spinner): thirty times a second, which keeps a soft sweep smooth without
/// redrawing the whole window at the display's rate.
const AMBIENT_FRAME: std::time::Duration = std::time::Duration::from_millis(33);

fn budget_id() -> egui::Id {
    egui::Id::new("tidalfast-power-budget")
}

/// Shares the app's drawing budget with widgets that have no `App` at hand.
pub fn set_budget(ctx: &egui::Context, budget: crate::power::Budget) {
    ctx.data_mut(|data| data.insert_temp(budget_id(), budget));
}

/// The drawing budget [`set_budget`] last shared; as designed until then.
pub fn budget(ctx: &egui::Context) -> crate::power::Budget {
    ctx.data(|data| data.get_temp(budget_id()))
        .unwrap_or_default()
}

/// Whether nobody can see the window: hidden, minimised, fully covered by
/// other windows, the display off or the session locked. Frames that only
/// move something on screen are not asked for then; the passes the logic
/// asks for still come and draw it as it stands.
pub fn unseen(ctx: &egui::Context) -> bool {
    budget(ctx) == crate::power::Budget::Background
}

/// Asks for an endless decoration's next frame: at `AMBIENT_FRAME`, half
/// that when out of focus or saving energy, and none when nobody can see
/// the window. Only call it while the decoration is on screen.
#[track_caller]
pub fn ambient_frame(ctx: &egui::Context) {
    use crate::power::Budget;
    match budget(ctx) {
        Budget::Normal => repaint_after(ctx, AMBIENT_FRAME),
        Budget::Saver => repaint_after(ctx, AMBIENT_FRAME * 2),
        Budget::Background => {}
    }
}

/// [`ambient_frame`] for a decoration that moves slowly and needs a frame
/// only every `interval` (a gradient drifting over thirty seconds).
#[track_caller]
pub fn ambient_frame_every(ctx: &egui::Context, interval: std::time::Duration) {
    use crate::power::Budget;
    match budget(ctx) {
        Budget::Normal => repaint_after(ctx, interval),
        Budget::Saver => repaint_after(ctx, interval * 2),
        Budget::Background => {}
    }
}

/// An animated busy indicator paced independently of the graphics driver.
pub fn spinner(ui: &mut egui::Ui, size: f32, color: Color32) -> Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(size), Sense::hover());
    if ui.is_rect_visible(rect) {
        ambient_frame(ui.ctx());
        let radius = size / 2.0 - 2.0;
        let start = ui.input(|input| input.time) * std::f64::consts::TAU * 1.2;
        let sweep = 250_f64.to_radians();
        let points = (0..20)
            .map(|index| {
                let angle = start + sweep * f64::from(index) / 19.0;
                let (sin, cos) = angle.sin_cos();
                rect.center() + radius * egui::vec2(cos as f32, sin as f32)
            })
            .collect();
        ui.painter()
            .add(egui::Shape::line(points, Stroke::new(2.0, color)));
    }
    response
}

/// Truncated single-line text in a given font and colour.
pub fn text(
    ui: &mut egui::Ui,
    text: impl Into<String>,
    font: egui::FontId,
    color: Color32,
) -> Response {
    let text = text.into();
    if crate::bidi::is_rtl(&text) {
        // Laid out here so a cut lands at the reading end, on the left.
        let galley = crate::bidi::layout(
            ui.painter(),
            &text,
            font,
            color,
            ui.available_width(),
            1,
            Some(crate::bidi::ELLIPSIS),
        );
        return ui.add(egui::Label::new(galley).selectable(false));
    }
    ui.add(
        egui::Label::new(egui::RichText::new(text).font(font).color(color))
            .truncate()
            .selectable(false),
    )
}

/// Single-line text that acts like a link: underlines on hover, clickable.
pub fn link(
    ui: &mut egui::Ui,
    text: impl Into<String>,
    font: egui::FontId,
    color: Color32,
) -> Response {
    let text = text.into();
    let response = if crate::bidi::is_rtl(&text) {
        let galley = crate::bidi::layout(
            ui.painter(),
            &text,
            font,
            color,
            ui.available_width(),
            1,
            Some(crate::bidi::ELLIPSIS),
        );
        ui.add(
            egui::Label::new(galley)
                .selectable(false)
                .sense(Sense::click()),
        )
    } else {
        ui.add(
            egui::Label::new(egui::RichText::new(text.clone()).font(font).color(color))
                .truncate()
                .selectable(false)
                .sense(Sense::click()),
        )
    };
    response
        .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Link, ui.is_enabled(), &text));
    focus_ring(ui, &response);
    if response.hovered() {
        let rect = response.rect;
        ui.painter()
            .hline(rect.x_range(), rect.bottom() - 1.0, Stroke::new(1.0, color));
    }
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

pub fn section_title(ui: &mut egui::Ui, palette: &Palette, label: &str) -> Response {
    text(ui, label, bold(17.0), palette.text)
}

pub fn subtle(ui: &mut egui::Ui, palette: &Palette, label: &str) -> Response {
    text(ui, label, regular(13.0), palette.secondary)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A theme's page font leads every weight, not only the regular one:
    /// Mocha Blue's `* { font-family: ... monospace }` is the titles' face
    /// in TIDAL too.
    #[test]
    fn the_pages_font_leads_every_weights_family() {
        let mut fonts = fastframe_fonts::FontSetup::default().definitions();
        let keys = fastframe_fonts::Weight::ALL.map(|w| format!("luna:Page:{}", w.name()));
        lead_with(&mut fonts, &keys);
        for weight in fastframe_fonts::Weight::ALL {
            let family = &fonts.families[&weight.family()];
            // Each weight leads with its own face of the page font.
            assert_eq!(
                family.first(),
                Some(&keys[weight_slot(weight)]),
                "{weight:?}"
            );
            assert!(family.len() > 1, "{weight:?} keeps its fallbacks");
        }
    }

    /// Every menu surface resolves to one colour: our menu frame and egui's
    /// own submenu, select-popup and tooltip frames. The sidebar flyout is
    /// built from the same frame.
    #[test]
    fn every_menu_surface_uses_the_one_menu_colour() {
        for palette in [Palette::dark(), Palette::light()] {
            let mut style = egui::Style::default();
            apply_to_style(&mut style, &palette);
            let menu = palette.menu_bg();
            assert_eq!(crate::ui::widgets::menu_frame(&palette).fill, menu);
            assert_eq!(egui::Frame::menu(&style).fill, menu, "submenus");
            assert_eq!(egui::Frame::popup(&style).fill, menu, "popups, selects");
        }
        assert_eq!(
            Palette::dark().menu_bg(),
            Color32::from_rgb(0x2d, 0x2d, 0x2d)
        );
    }

    /// No view paints its own menu grey: the colour lives in the palette.
    #[test]
    fn views_do_not_hard_code_the_menu_grey() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/ui");
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.extension().is_some_and(|e| e == "rs") {
                let text = std::fs::read_to_string(&path).unwrap();
                for grey in ["0x2d, 0x2d, 0x2d", "0x1c, 0x1c, 0x1f"] {
                    assert!(!text.contains(grey), "{} paints {grey}", path.display());
                }
            }
        }
    }

    /// The logo drawn in the app is the app icon's glyph without its square,
    /// white so the colour passed in tints it: it uploads the glyph's pixels.
    #[test]
    fn the_logo_in_the_app_is_the_icons_glyph() {
        // #given the logo drawn 40 points wide at twice the pixel density
        let ctx = egui::Context::default();
        ctx.set_pixels_per_point(2.0);
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            logo(ui, egui::pos2(40.0, 40.0), 40.0, Color32::WHITE);
        });

        // #then the frame uploads the glyph rasterised at that size
        let uploaded = output
            .textures_delta
            .set
            .values()
            .flat_map(|deltas| deltas.iter())
            .find_map(|delta| match &delta.image {
                egui::ImageData::Color(image) if image.size == [84, 84] => Some(image.clone()),
                _ => None,
            })
            .expect("the logo's texture");
        assert!(
            uploaded.pixels.iter().any(|pixel| pixel.a() == 255),
            "the glyph shows"
        );
        let icon = crate::util::logo_glyph_rgba(84);
        for (x, y) in [(42, 10), (47, 42), (42, 4)] {
            let expected = &icon[(y * 84 + x) * 4..(y * 84 + x) * 4 + 4];
            let got = uploaded.pixels[y * 84 + x].to_srgba_unmultiplied();
            // A clear pixel has no colour of its own to compare.
            if expected[3] == 0 {
                assert_eq!(got[3], 0, "pixel {x},{y}");
            } else {
                assert_eq!(&got[..], expected, "pixel {x},{y}");
            }
        }
        output.textures_delta.clear();
    }

    /// A palette replaces egui's visuals, which must not take back the
    /// desktop's text rendering: on Linux, linear coverage in both themes.
    #[test]
    fn palettes_keep_the_desktops_text_rendering() {
        for palette in [Palette::dark(), Palette::light()] {
            let ctx = egui::Context::default();
            apply(&ctx, &palette);
            let options = ctx.global_style().visuals.text_options;
            assert_eq!(
                options.color_transfer_function,
                text_rendering().color_transfer_function(palette.dark),
                "dark: {}",
                palette.dark
            );
            if cfg!(target_os = "linux") {
                assert_eq!(
                    options.color_transfer_function,
                    egui::epaint::FontColorTransferFunction::Off
                );
            }
            assert!(options.subpixel_binning);
        }
    }

    /// Buttons leave the cursor alone (`ui::cursor` gives every clickable
    /// TIDAL's hand once the frame is drawn); links ask for the hand
    /// themselves, so it shows wherever they are drawn.
    #[test]
    fn only_links_ask_for_the_hand_cursor_themselves() {
        let ctx = egui::Context::default();
        install(&ctx);
        let palette = Palette::dark();
        let draw = |ctx: &egui::Context, pointer: Option<egui::Pos2>| {
            let mut rects = (egui::Rect::NOTHING, egui::Rect::NOTHING);
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(400.0, 200.0),
                )),
                events: pointer
                    .map(|at| vec![egui::Event::PointerMoved(at)])
                    .unwrap_or_default(),
                ..Default::default()
            };
            let mut output = ctx.run_ui(input, |ui| {
                rects.0 = pill_button(ui, &palette, "Play", true).rect;
                rects.1 = link(ui, "Bonobo", regular(14.0), palette.text).rect;
            });
            output.textures_delta.clear();
            (rects, output.platform_output.cursor_icon)
        };
        draw(&ctx, None);
        let ((button, link_rect), _) = draw(&ctx, None);
        let (_, over_button) = draw(&ctx, Some(button.center()));
        assert_eq!(over_button, egui::CursorIcon::Default);
        let (_, over_link) = draw(&ctx, Some(link_rect.center()));
        assert_eq!(over_link, egui::CursorIcon::PointingHand);
    }

    #[test]
    fn a_local_palette_keeps_tidalfasts_widget_style_local() {
        let ctx = egui::Context::default();
        apply(&ctx, &Palette::light());
        let dark = Palette::dark();
        let mut output = ctx.run_ui(Default::default(), |ui| {
            apply_local(ui, &dark);
            let style = ui.style();
            assert!(style.visuals.dark_mode);
            assert_eq!(
                style.visuals.widgets.inactive.corner_radius,
                CornerRadius::same(RADIUS_SMALL + 2)
            );
            assert_eq!(
                style.visuals.selection.stroke,
                Stroke::new(1.0, dark.accent)
            );
            assert_eq!(style.spacing.button_padding, Vec2::new(12.0, 6.0));
        });
        output.textures_delta.clear();
        assert!(!ctx.global_style().visuals.dark_mode);
    }

    #[test]
    fn fonts_install_and_layout_emojis() {
        let ctx = egui::Context::default();
        install(&ctx);
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            let galley = ui.painter().layout_no_wrap(
                "Rosewood 🔥 Otomo 🎵 ❤️ 🚀".to_string(),
                regular(14.0),
                Color32::WHITE,
            );
            assert!(galley.rows[0].glyphs.len() >= 5);
        });
        output.textures_delta.clear();
    }

    /// The monochrome emoji face comes right after Hanken at every weight
    /// and in the monospace family, ahead of egui's own emoji pair.
    #[test]
    fn the_emoji_face_follows_hanken_everywhere() {
        let ctx = egui::Context::default();
        install(&ctx);
        ctx.run_ui(egui::RawInput::default(), |_| {})
            .textures_delta
            .clear();
        let fonts = ctx.fonts(|fonts| fonts.definitions().clone());
        for weight in fastframe_fonts::Weight::ALL {
            let family = &fonts.families[&weight.family()];
            assert_eq!(family[..2], [weight.name(), "noto_emoji"], "{weight:?}");
        }
        assert_eq!(
            fonts.families[&egui::FontFamily::Monospace][1],
            "noto_emoji"
        );
    }

    #[test]
    fn fonts_install_and_layout_mixed_cjk() {
        let ctx = egui::Context::default();
        install(&ctx);
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            let galley = ui.painter().layout_no_wrap(
                "Track 87: 恋におちて -Fall in love- (Live)".to_string(),
                regular(14.0),
                Color32::WHITE,
            );
            assert!(!galley.rows.is_empty());
            assert!(galley.rows[0].glyphs.len() >= 10);
        });
        output.textures_delta.clear();
    }

    /// An animation asking for its next frame in 33 ms gets it in 33 ms, not
    /// in the 16 ms left after egui's predicted frame.
    #[test]
    fn a_timed_repaint_waits_the_whole_interval() {
        let ctx = egui::Context::default();
        let delays = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let seen = std::sync::Arc::clone(&delays);
        ctx.set_request_repaint_callback(move |info| seen.lock().unwrap().push(info.delay));
        repaint_after(&ctx, std::time::Duration::from_millis(33));
        let delays = delays.lock().unwrap();
        assert_eq!(delays.len(), 1);
        let ms = delays[0].as_secs_f64() * 1_000.0;
        assert!((ms - 33.0).abs() < 0.5, "{ms} ms");
    }

    /// An endless decoration runs at thirty frames a second, fifteen out of
    /// focus, and asks for nothing when nobody can see the window.
    #[test]
    fn an_ambient_animation_follows_the_budget() {
        let ctx = egui::Context::default();
        let delays = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let seen = std::sync::Arc::clone(&delays);
        ctx.set_request_repaint_callback(move |info| seen.lock().unwrap().push(info.delay));
        ambient_frame(&ctx);
        set_budget(&ctx, crate::power::Budget::Background);
        ambient_frame(&ctx);
        let delays = delays.lock().unwrap();
        assert_eq!(delays.len(), 1, "nothing asked for in the background");
        let ms = delays[0].as_secs_f64() * 1_000.0;
        assert!((ms - 33.0).abs() < 0.5, "{ms} ms");
    }
}
