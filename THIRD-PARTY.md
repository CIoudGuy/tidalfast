# Third-party software

Tidalfast is built from these Rust crates on Windows, Linux and macOS.
Each is used under the license shown; the full texts are in each crate's
repository and package. Fonts and icons in `assets/` carry their own
license files next to them.

Generated with `cargo tree --target all -e normal,build --prefix none --format "{p};{l};{r}"`.

| Crate | Version | License |
|---|---|---|
| [accesskit](https://github.com/AccessKit/accesskit) | 0.24.1 | MIT OR Apache-2.0 |
| [accesskit_atspi_common](https://github.com/AccessKit/accesskit) | 0.18.1 | MIT OR Apache-2.0 |
| [accesskit_consumer](https://github.com/AccessKit/accesskit) | 0.35.0 | MIT OR Apache-2.0 |
| [accesskit_consumer](https://github.com/AccessKit/accesskit) | 0.36.0 | MIT OR Apache-2.0 |
| [accesskit_consumer](https://github.com/AccessKit/accesskit) | 0.38.0 | MIT OR Apache-2.0 |
| [accesskit_macos](https://github.com/AccessKit/accesskit) | 0.26.3 | MIT OR Apache-2.0 |
| [accesskit_unix](https://github.com/AccessKit/accesskit) | 0.21.1 | MIT OR Apache-2.0 |
| [accesskit_windows](https://github.com/AccessKit/accesskit) | 0.32.1 | MIT OR Apache-2.0 |
| [accesskit_winit](https://github.com/AccessKit/accesskit) | 0.32.2 | Apache-2.0 |
| [adler2](https://github.com/oyvindln/adler2) | 2.0.1 | 0BSD OR MIT OR Apache-2.0 |
| [aes](https://github.com/RustCrypto/block-ciphers) | 0.9.3 | MIT OR Apache-2.0 |
| [ahash](https://github.com/tkaitchuck/ahash) | 0.8.12 | MIT OR Apache-2.0 |
| [allocator-api2](https://github.com/zakarumych/allocator-api2) | 0.2.21 | MIT OR Apache-2.0 |
| [alsa](https://github.com/diwic/alsa-rs) | 0.11.0 | Apache-2.0/MIT |
| [alsa-sys](https://github.com/diwic/alsa-sys) | 0.4.0 | MIT |
| [android-activity](https://github.com/rust-mobile/android-activity) | 0.6.1 | MIT OR Apache-2.0 |
| [android-properties](https://github.com/miklelappo/android-properties) | 0.2.2 | MIT |
| [anstream](https://github.com/rust-cli/anstyle.git) | 1.0.0 | MIT OR Apache-2.0 |
| [anstyle](https://github.com/rust-cli/anstyle.git) | 1.0.14 | MIT OR Apache-2.0 |
| [anstyle-parse](https://github.com/rust-cli/anstyle.git) | 1.0.0 | MIT OR Apache-2.0 |
| [anstyle-query](https://github.com/rust-cli/anstyle.git) | 1.1.5 | MIT OR Apache-2.0 |
| [anstyle-wincon](https://github.com/rust-cli/anstyle.git) | 3.0.11 | MIT OR Apache-2.0 |
| [anyhow](https://github.com/dtolnay/anyhow) | 1.0.104 | MIT OR Apache-2.0 |
| [apple-native-keyring-store](https://github.com/open-source-cooperative/apple-native-keyring-store.git) | 1.0.2 | MIT OR Apache-2.0 |
| [arboard](https://github.com/1Password/arboard) | 3.6.1 | MIT OR Apache-2.0 |
| [arrayref](https://github.com/droundy/arrayref) | 0.3.9 | BSD-2-Clause |
| [arrayvec](https://github.com/bluss/arrayvec) | 0.7.8 | MIT OR Apache-2.0 |
| [as-raw-xcb-connection](https://github.com/psychon/as-raw-xcb-connection) | 1.0.1 | MIT OR Apache-2.0 |
| [async-broadcast](https://github.com/smol-rs/async-broadcast) | 0.7.2 | MIT OR Apache-2.0 |
| [async-channel](https://github.com/smol-rs/async-channel) | 2.5.0 | Apache-2.0 OR MIT |
| [async-compression](https://github.com/Nullus157/async-compression) | 0.4.43 | MIT OR Apache-2.0 |
| [async-executor](https://github.com/smol-rs/async-executor) | 1.14.0 | Apache-2.0 OR MIT |
| [async-io](https://github.com/smol-rs/async-io) | 2.6.0 | Apache-2.0 OR MIT |
| [async-lock](https://github.com/smol-rs/async-lock) | 3.4.2 | Apache-2.0 OR MIT |
| [async-process](https://github.com/smol-rs/async-process) | 2.5.0 | Apache-2.0 OR MIT |
| [async-recursion](https://github.com/dcchut/async-recursion) | 1.1.1 | MIT OR Apache-2.0 |
| [async-signal](https://github.com/smol-rs/async-signal) | 0.2.14 | Apache-2.0 OR MIT |
| [async-task](https://github.com/smol-rs/async-task) | 4.7.1 | Apache-2.0 OR MIT |
| [async-trait](https://github.com/dtolnay/async-trait) | 0.1.92 | MIT OR Apache-2.0 |
| [atomic-waker](https://github.com/smol-rs/atomic-waker) | 1.1.2 | Apache-2.0 OR MIT |
| [atspi](https://github.com/odilia-app/atspi) | 0.29.0 | Apache-2.0 OR MIT |
| [atspi-common](https://github.com/odilia-app/atspi) | 0.13.0 | Apache-2.0 OR MIT |
| [atspi-proxies](https://github.com/odilia-app/atspi) | 0.13.0 | Apache-2.0 OR MIT |
| [autocfg](https://github.com/cuviper/autocfg) | 1.5.1 | Apache-2.0 OR MIT |
| [base64](https://github.com/marshallpierce/rust-base64) | 0.22.1 | MIT OR Apache-2.0 |
| [bitflags](https://github.com/bitflags/bitflags) | 1.3.2 | MIT/Apache-2.0 |
| [bitflags](https://github.com/bitflags/bitflags) | 2.13.1 | MIT OR Apache-2.0 |
| [block](http://github.com/SSheldon/rust-block) | 0.1.6 | MIT |
| [block-buffer](https://github.com/RustCrypto/utils) | 0.10.4 | MIT OR Apache-2.0 |
| [block-buffer](https://github.com/RustCrypto/utils) | 0.12.1 | MIT OR Apache-2.0 |
| [block-padding](https://github.com/RustCrypto/utils) | 0.4.2 | MIT OR Apache-2.0 |
| [block2](https://github.com/madsmtm/objc2) | 0.5.1 | MIT |
| [block2](https://github.com/madsmtm/objc2) | 0.6.2 | MIT |
| [blocking](https://github.com/smol-rs/blocking) | 1.7.0 | Apache-2.0 OR MIT |
| [bumpalo](https://github.com/fitzgen/bumpalo) | 3.20.3 | MIT OR Apache-2.0 |
| [bytemuck](https://github.com/Lokathor/bytemuck) | 1.25.2 | Zlib OR Apache-2.0 OR MIT |
| [bytemuck_derive](https://github.com/Lokathor/bytemuck) | 1.12.0 | Zlib OR Apache-2.0 OR MIT |
| [byteorder](https://github.com/BurntSushi/byteorder) | 1.5.0 | Unlicense OR MIT |
| [byteorder-lite](https://github.com/image-rs/byteorder-lite) | 0.1.0 | Unlicense OR MIT |
| [bytes](https://github.com/tokio-rs/bytes) | 1.12.1 | MIT |
| [bzip2](https://github.com/trifectatechfoundation/bzip2-rs) | 0.6.1 | MIT OR Apache-2.0 |
| [calloop](https://github.com/Smithay/calloop) | 0.13.0 | MIT |
| [calloop](https://github.com/Smithay/calloop) | 0.14.4 | MIT |
| [calloop-wayland-source](https://github.com/smithay/calloop-wayland-source) | 0.3.0 | MIT |
| [calloop-wayland-source](https://github.com/smithay/calloop-wayland-source) | 0.4.1 | MIT |
| [cbc](https://github.com/RustCrypto/block-modes) | 0.2.1 | MIT OR Apache-2.0 |
| [cc](https://github.com/rust-lang/cc-rs) | 1.4.4 | MIT OR Apache-2.0 |
| [cfg-if](https://github.com/rust-lang/cfg-if) | 1.0.4 | MIT OR Apache-2.0 |
| [cfg_aliases](https://github.com/katharostech/cfg_aliases) | 0.2.2 | MIT |
| [cgl](https://github.com/servo/cgl-rs) | 0.3.2 | MIT / Apache-2.0 |
| [cipher](https://github.com/RustCrypto/traits) | 0.5.2 | MIT OR Apache-2.0 |
| [clap](https://github.com/clap-rs/clap) | 4.6.6 | MIT OR Apache-2.0 |
| [clap_builder](https://github.com/clap-rs/clap) | 4.6.6 | MIT OR Apache-2.0 |
| [clap_derive](https://github.com/clap-rs/clap) | 4.6.4 | MIT OR Apache-2.0 |
| [clap_lex](https://github.com/clap-rs/clap) | 1.1.0 | MIT OR Apache-2.0 |
| [clipboard-win](https://github.com/DoumanAsh/clipboard-win) | 5.4.1 | BSL-1.0 |
| [cmov](https://github.com/RustCrypto/utils) | 0.5.4 | Apache-2.0 OR MIT |
| [cocoa](https://github.com/servo/core-foundation-rs) | 0.24.1 | MIT / Apache-2.0 |
| [cocoa-foundation](https://github.com/servo/core-foundation-rs) | 0.1.2 | MIT OR Apache-2.0 |
| [color](https://github.com/linebender/color) | 0.3.3 | Apache-2.0 OR MIT |
| [color_quant](https://github.com/image-rs/color_quant.git) | 1.1.0 | MIT |
| [colorchoice](https://github.com/rust-cli/anstyle.git) | 1.0.5 | MIT OR Apache-2.0 |
| [combine](https://github.com/Marwes/combine) | 4.6.8 | MIT |
| [compression-codecs](https://github.com/Nullus157/async-compression) | 0.4.38 | MIT OR Apache-2.0 |
| [compression-core](https://github.com/Nullus157/async-compression) | 0.4.32 | MIT OR Apache-2.0 |
| [concat-string](https://github.com/FaultyRAM/concat-string) | 1.0.1 | Apache-2.0 OR MIT |
| [concurrent-queue](https://github.com/smol-rs/concurrent-queue) | 2.5.0 | Apache-2.0 OR MIT |
| [const-oid](https://github.com/RustCrypto/formats) | 0.10.2 | Apache-2.0 OR MIT |
| [core-foundation](https://github.com/servo/core-foundation-rs) | 0.10.1 | MIT OR Apache-2.0 |
| [core-foundation](https://github.com/servo/core-foundation-rs) | 0.9.4 | MIT OR Apache-2.0 |
| [core-foundation-sys](https://github.com/servo/core-foundation-rs) | 0.8.7 | MIT OR Apache-2.0 |
| [core-graphics](https://github.com/servo/core-foundation-rs) | 0.22.3 | MIT / Apache-2.0 |
| [core-graphics](https://github.com/servo/core-foundation-rs) | 0.23.2 | MIT OR Apache-2.0 |
| [core-graphics-types](https://github.com/servo/core-foundation-rs) | 0.1.3 | MIT OR Apache-2.0 |
| [coreaudio-rs](https://github.com/RustAudio/coreaudio-rs.git) | 0.14.2 | MIT/Apache-2.0 |
| [cpal](https://github.com/RustAudio/cpal) | 0.18.2 | Apache-2.0 |
| [cpubits](https://github.com/RustCrypto/utils) | 0.1.1 | MIT OR Apache-2.0 |
| [cpufeatures](https://github.com/RustCrypto/utils) | 0.2.17 | MIT OR Apache-2.0 |
| [cpufeatures](https://github.com/RustCrypto/utils) | 0.3.0 | MIT OR Apache-2.0 |
| [crc32fast](https://github.com/srijs/rust-crc32fast) | 1.5.1 | MIT OR Apache-2.0 |
| [crossbeam-channel](https://github.com/crossbeam-rs/crossbeam) | 0.5.16 | MIT OR Apache-2.0 |
| [crossbeam-utils](https://github.com/crossbeam-rs/crossbeam) | 0.8.22 | MIT OR Apache-2.0 |
| [crunchy](https://github.com/eira-fransham/crunchy) | 0.2.4 | MIT |
| [crypto-common](https://github.com/RustCrypto/traits) | 0.1.7 | MIT OR Apache-2.0 |
| [crypto-common](https://github.com/RustCrypto/traits) | 0.2.2 | MIT OR Apache-2.0 |
| [ctutils](https://github.com/RustCrypto/utils) | 0.4.2 | Apache-2.0 OR MIT |
| [cursor-icon](https://github.com/rust-windowing/cursor-icon) | 1.2.0 | MIT OR Apache-2.0 OR Zlib |
| [dasp_sample](https://github.com/rustaudio/sample.git) | 0.11.0 | MIT OR Apache-2.0 |
| [data-url](https://github.com/servo/rust-url) | 0.3.2 | MIT OR Apache-2.0 |
| [digest](https://github.com/RustCrypto/traits) | 0.10.7 | MIT OR Apache-2.0 |
| [digest](https://github.com/RustCrypto/traits) | 0.11.3 | MIT OR Apache-2.0 |
| [directories](https://github.com/soc/directories-rs) | 6.0.0 | MIT OR Apache-2.0 |
| [dirs-sys](https://github.com/dirs-dev/dirs-sys-rs) | 0.5.0 | MIT OR Apache-2.0 |
| [dispatch](http://github.com/SSheldon/rust-dispatch) | 0.2.0 | MIT |
| [dispatch2](https://github.com/madsmtm/objc2) | 0.3.1 | Zlib OR Apache-2.0 OR MIT |
| [displaydoc](https://github.com/yaahc/displaydoc) | 0.2.7 | MIT OR Apache-2.0 |
| [dlib](https://github.com/elinorbgr/dlib) | 0.5.3 | MIT |
| [document-features](https://github.com/slint-ui/document-features) | 0.2.12 | MIT OR Apache-2.0 |
| [downcast-rs](https://github.com/marcianx/downcast-rs) | 1.2.1 | MIT/Apache-2.0 |
| [dpi](https://github.com/rust-windowing/winit) | 0.1.1 | Apache-2.0 |
| [dpi](https://github.com/rust-windowing/winit) | 0.1.2 | Apache-2.0 AND MIT |
| [ecolor](https://github.com/emilk/egui) | 0.36.1 | MIT OR Apache-2.0 |
| [eframe](https://github.com/emilk/egui/tree/main/crates/eframe) | 0.36.1 | MIT OR Apache-2.0 |
| [egui](https://github.com/emilk/egui) | 0.36.1 | MIT OR Apache-2.0 |
| [egui-winit](https://github.com/emilk/egui/tree/main/crates/egui-winit) | 0.36.1 | MIT OR Apache-2.0 |
| [egui_extras](https://github.com/emilk/egui) | 0.36.1 | MIT OR Apache-2.0 |
| [egui_glow](https://github.com/emilk/egui/tree/main/crates/egui_glow) | 0.36.1 | MIT OR Apache-2.0 |
| [either](https://github.com/rayon-rs/either) | 1.18.0 | MIT OR Apache-2.0 |
| [emath](https://github.com/emilk/egui/tree/main/crates/emath) | 0.36.1 | MIT OR Apache-2.0 |
| [encoding_rs](https://github.com/hsivonen/encoding_rs) | 0.8.35 | (Apache-2.0 OR MIT) AND BSD-3-Clause |
| [endi](https://github.com/zeenix/endi) | 1.1.1 | MIT |
| [enum-map](https://codeberg.org/xfix/enum-map) | 2.7.3 | MIT OR Apache-2.0 |
| [enum-map-derive](https://codeberg.org/xfix/enum-map) | 0.17.0 | MIT OR Apache-2.0 |
| [enumflags2](https://github.com/meithecatte/enumflags2) | 0.7.12 | MIT OR Apache-2.0 |
| [enumflags2_derive](https://github.com/meithecatte/enumflags2) | 0.7.12 | MIT OR Apache-2.0 |
| [enumn](https://github.com/dtolnay/enumn) | 0.1.14 | MIT OR Apache-2.0 |
| [env_filter](https://github.com/rust-cli/env_logger) | 2.0.0 | MIT OR Apache-2.0 |
| [env_logger](https://github.com/rust-cli/env_logger) | 0.11.11 | MIT OR Apache-2.0 |
| [epaint](https://github.com/emilk/egui/tree/main/crates/epaint) | 0.36.1 | MIT OR Apache-2.0 |
| [epaint_default_fonts](https://github.com/emilk/egui/tree/main/crates/epaint_default_fonts) | 0.36.1 | (MIT OR Apache-2.0) AND OFL-1.1 AND Ubuntu-font-1.0 |
| [equivalent](https://github.com/indexmap-rs/equivalent) | 1.0.2 | Apache-2.0 OR MIT |
| [errno](https://github.com/lambda-fairy/rust-errno) | 0.3.14 | MIT OR Apache-2.0 |
| [error-code](https://github.com/DoumanAsh/error-code) | 3.4.0 | BSL-1.0 |
| [euclid](https://github.com/servo/euclid) | 0.22.14 | MIT OR Apache-2.0 |
| [event-listener](https://github.com/smol-rs/event-listener) | 5.4.2 | Apache-2.0 OR MIT |
| [event-listener-strategy](https://github.com/smol-rs/event-listener-strategy) | 0.5.4 | Apache-2.0 OR MIT |
| [extended](https://github.com/depp/extended-rs) | 0.1.0 | MIT |
| [fastframe-audio](https://github.com/crmne/fastframe) | 0.5.1 | MIT |
| [fastframe-desktop](https://github.com/crmne/fastframe) | 0.5.1 | MIT |
| [fastframe-emoji](https://github.com/crmne/fastframe) | 0.5.1 | MIT |
| [fastframe-file](https://github.com/crmne/fastframe) | 0.5.1 | MIT |
| [fastframe-fonts](https://github.com/crmne/fastframe) | 0.5.1 | MIT AND OFL-1.1 |
| [fastframe-i18n](https://github.com/crmne/fastframe) | 0.5.1 | MIT |
| [fastframe-icons](https://github.com/crmne/fastframe) | 0.5.1 | MIT AND ISC |
| [fastframe-instance](https://github.com/crmne/fastframe) | 0.5.1 | MIT |
| [fastframe-log](https://github.com/crmne/fastframe) | 0.5.1 | MIT |
| [fastframe-macos](https://github.com/crmne/fastframe) | 0.5.1 | MIT |
| [fastframe-now-playing](https://github.com/crmne/fastframe) | 0.5.1 | MIT |
| [fastframe-power](https://github.com/crmne/fastframe) | 0.5.1 | MIT |
| [fastframe-scroll](https://github.com/crmne/fastframe) | 0.5.1 | MIT |
| [fastframe-shell](https://github.com/crmne/fastframe) | 0.5.1 | MIT |
| [fastframe-text](https://github.com/crmne/fastframe) | 0.5.1 | MIT |
| [fastframe-theme](https://github.com/crmne/fastframe) | 0.5.1 | MIT |
| [fastframe-tray](https://github.com/crmne/fastframe) | 0.5.1 | MIT |
| [fastframe-update](https://github.com/crmne/fastframe) | 0.5.1 | MIT |
| [fastrand](https://github.com/smol-rs/fastrand) | 2.5.0 | Apache-2.0 OR MIT |
| [fax](https://github.com/pdf-rs/fax) | 0.2.7 | MIT |
| [fdeflate](https://github.com/image-rs/fdeflate) | 0.3.7 | MIT OR Apache-2.0 |
| [fearless_simd](https://github.com/linebender/fearless_simd) | 0.4.1 | Apache-2.0 OR MIT |
| [find-msvc-tools](https://github.com/rust-lang/cc-rs) | 0.1.11 | MIT OR Apache-2.0 |
| [flate2](https://github.com/rust-lang/flate2-rs) | 1.1.9 | MIT OR Apache-2.0 |
| [float-cmp](https://github.com/mikedilger/float-cmp) | 0.9.0 | MIT |
| [fnv](https://github.com/servo/rust-fnv) | 1.0.7 | Apache-2.0 / MIT |
| [foldhash](https://github.com/orlp/foldhash) | 0.2.0 | Zlib |
| [font-types](https://github.com/googlefonts/fontations) | 0.12.4 | MIT OR Apache-2.0 |
| [foreign-types](https://github.com/sfackler/foreign-types) | 0.3.2 | MIT/Apache-2.0 |
| [foreign-types](https://github.com/sfackler/foreign-types) | 0.5.0 | MIT/Apache-2.0 |
| [foreign-types-macros](https://github.com/sfackler/foreign-types) | 0.2.4 | MIT/Apache-2.0 |
| [foreign-types-shared](https://github.com/sfackler/foreign-types) | 0.1.1 | MIT/Apache-2.0 |
| [foreign-types-shared](https://github.com/sfackler/foreign-types) | 0.3.1 | MIT/Apache-2.0 |
| [form_urlencoded](https://github.com/servo/rust-url) | 1.2.2 | MIT OR Apache-2.0 |
| [fsevent-sys](https://github.com/octplane/fsevent-rust/tree/master/fsevent-sys) | 4.1.0 | MIT |
| [futures-channel](https://github.com/rust-lang/futures-rs) | 0.3.34 | MIT OR Apache-2.0 |
| [futures-core](https://github.com/rust-lang/futures-rs) | 0.3.34 | MIT OR Apache-2.0 |
| [futures-io](https://github.com/rust-lang/futures-rs) | 0.3.34 | MIT OR Apache-2.0 |
| [futures-lite](https://github.com/smol-rs/futures-lite) | 2.6.1 | Apache-2.0 OR MIT |
| [futures-macro](https://github.com/rust-lang/futures-rs) | 0.3.34 | MIT OR Apache-2.0 |
| [futures-sink](https://github.com/rust-lang/futures-rs) | 0.3.34 | MIT OR Apache-2.0 |
| [futures-task](https://github.com/rust-lang/futures-rs) | 0.3.34 | MIT OR Apache-2.0 |
| [futures-util](https://github.com/rust-lang/futures-rs) | 0.3.34 | MIT OR Apache-2.0 |
| [generic-array](https://github.com/fizyk20/generic-array.git) | 0.14.7 | MIT |
| [gethostname](https://codeberg.org/swsnr/gethostname.rs.git) | 1.1.0 | Apache-2.0 |
| [getrandom](https://github.com/rust-random/getrandom) | 0.2.17 | MIT OR Apache-2.0 |
| [getrandom](https://github.com/rust-random/getrandom) | 0.3.4 | MIT OR Apache-2.0 |
| [getrandom](https://github.com/rust-random/getrandom) | 0.4.3 | MIT OR Apache-2.0 |
| [gif](https://github.com/image-rs/image-gif) | 0.14.2 | MIT OR Apache-2.0 |
| [gl_generator](https://github.com/brendanzab/gl-rs/) | 0.14.0 | Apache-2.0 |
| [glow](https://github.com/grovesNL/glow) | 0.17.0 | MIT OR Apache-2.0 OR Zlib |
| [glutin](https://github.com/rust-windowing/glutin) | 0.32.3 | Apache-2.0 |
| [glutin-winit](https://github.com/rust-windowing/glutin) | 0.5.0 | MIT |
| [glutin_egl_sys](https://github.com/rust-windowing/glutin) | 0.7.1 | Apache-2.0 |
| [glutin_glx_sys](https://github.com/rust-windowing/glutin) | 0.6.1 | Apache-2.0 |
| [glutin_wgl_sys](https://github.com/rust-windowing/glutin) | 0.6.1 | Apache-2.0 |
| [guillotiere](https://github.com/nical/guillotiere) | 0.7.0 | MIT/Apache-2.0 |
| [h2](https://github.com/hyperium/h2) | 0.4.19 | MIT |
| [half](https://github.com/VoidStarKat/half-rs) | 2.7.1 | MIT OR Apache-2.0 |
| [harfrust](https://github.com/harfbuzz/harfrust) | 0.12.0 | MIT |
| [hashbrown](https://github.com/rust-lang/hashbrown) | 0.16.1 | MIT OR Apache-2.0 |
| [hashbrown](https://github.com/rust-lang/hashbrown) | 0.17.1 | MIT OR Apache-2.0 |
| [heck](https://github.com/withoutboats/heck) | 0.5.0 | MIT OR Apache-2.0 |
| [hermit-abi](https://github.com/hermit-os/hermit-rs) | 0.5.2 | MIT OR Apache-2.0 |
| [hex](https://github.com/KokaKiwi/rust-hex) | 0.4.3 | MIT OR Apache-2.0 |
| [hkdf](https://github.com/RustCrypto/KDFs/) | 0.13.0 | MIT OR Apache-2.0 |
| [hmac](https://github.com/RustCrypto/MACs) | 0.12.1 | MIT OR Apache-2.0 |
| [hmac](https://github.com/RustCrypto/MACs) | 0.13.0 | MIT OR Apache-2.0 |
| [http](https://github.com/hyperium/http) | 1.5.0 | MIT OR Apache-2.0 |
| [http-body](https://github.com/hyperium/http-body) | 1.1.0 | MIT |
| [http-body-util](https://github.com/hyperium/http-body) | 0.1.5 | MIT |
| [httparse](https://github.com/seanmonstar/httparse) | 1.10.1 | MIT OR Apache-2.0 |
| [hybrid-array](https://github.com/RustCrypto/hybrid-array) | 0.4.15 | MIT OR Apache-2.0 |
| [hyper](https://github.com/hyperium/hyper) | 1.11.0 | MIT |
| [hyper-rustls](https://github.com/rustls/hyper-rustls) | 0.27.9 | Apache-2.0 OR ISC OR MIT |
| [hyper-util](https://github.com/hyperium/hyper-util) | 0.1.20 | MIT |
| [icu_collections](https://github.com/unicode-org/icu4x) | 2.3.0 | Unicode-3.0 |
| [icu_locale_core](https://github.com/unicode-org/icu4x) | 2.3.0 | Unicode-3.0 |
| [icu_normalizer](https://github.com/unicode-org/icu4x) | 2.3.0 | Unicode-3.0 |
| [icu_normalizer_data](https://github.com/unicode-org/icu4x) | 2.3.0 | Unicode-3.0 |
| [icu_properties](https://github.com/unicode-org/icu4x) | 2.3.0 | Unicode-3.0 |
| [icu_properties_data](https://github.com/unicode-org/icu4x) | 2.3.0 | Unicode-3.0 |
| [icu_provider](https://github.com/unicode-org/icu4x) | 2.3.1 | Unicode-3.0 |
| [idna](https://github.com/servo/rust-url/) | 1.1.0 | MIT OR Apache-2.0 |
| [idna_adapter](https://github.com/hsivonen/idna_adapter) | 1.2.2 | Apache-2.0 OR MIT |
| [image](https://github.com/image-rs/image) | 0.25.10 | MIT OR Apache-2.0 |
| [image-webp](https://github.com/image-rs/image-webp) | 0.2.4 | MIT OR Apache-2.0 |
| [imagesize](https://github.com/Roughsketch/imagesize) | 0.13.0 | MIT |
| [include-po](https://github.com/rodrigorc/include-po) | 0.2.1 | MIT |
| [indexmap](https://github.com/indexmap-rs/indexmap) | 2.14.0 | Apache-2.0 OR MIT |
| [inotify](https://github.com/hannobraun/inotify-rs) | 0.11.5 | ISC |
| [inotify-sys](https://github.com/hannobraun/inotify-sys) | 0.1.8 | ISC |
| [inout](https://github.com/RustCrypto/utils) | 0.2.2 | MIT OR Apache-2.0 |
| [ipnet](https://github.com/krisprice/ipnet) | 2.12.1 | MIT OR Apache-2.0 |
| [is-docker](https://github.com/TheLarkInn/is-docker) | 0.2.0 | MIT |
| [is-wsl](https://github.com/TheLarkInn/is-wsl) | 0.4.0 | MIT |
| [is_terminal_polyfill](https://github.com/polyfill-rs/is_terminal_polyfill) | 1.70.2 | MIT OR Apache-2.0 |
| [itertools](https://github.com/rust-itertools/itertools) | 0.15.0 | MIT OR Apache-2.0 |
| [itoa](https://github.com/dtolnay/itoa) | 1.0.18 | MIT OR Apache-2.0 |
| [jiff](https://github.com/BurntSushi/jiff) | 0.2.35 | Unlicense OR MIT |
| [jiff-core](https://github.com/BurntSushi/jiff) | 0.1.0 | Unlicense OR MIT |
| [jiff-static](https://github.com/BurntSushi/jiff) | 0.2.35 | Unlicense OR MIT |
| [jiff-tzdb](https://github.com/BurntSushi/jiff) | 0.1.8 | Unlicense OR MIT |
| [jiff-tzdb-platform](https://github.com/BurntSushi/jiff) | 0.1.3 | Unlicense OR MIT |
| [jni](https://github.com/jni-rs/jni-rs) | 0.22.4 | MIT OR Apache-2.0 |
| [jni-macros](https://github.com/jni-rs/jni-rs) | 0.22.4 | MIT OR Apache-2.0 |
| [jni-sys](https://github.com/jni-rs/jni-sys) | 0.3.1 | MIT OR Apache-2.0 |
| [jni-sys](https://github.com/jni-rs/jni-sys) | 0.4.1 | MIT OR Apache-2.0 |
| [jni-sys-macros](https://github.com/jni-rs/jni-sys) | 0.4.1 | MIT OR Apache-2.0 |
| [jobserver](https://github.com/rust-lang/jobserver-rs) | 0.1.35 | MIT OR Apache-2.0 |
| [js-sys](https://github.com/wasm-bindgen/wasm-bindgen/tree/master/crates/js-sys) | 0.3.104 | MIT OR Apache-2.0 |
| [keyboard-types](https://github.com/rust-windowing/keyboard-types) | 0.8.3 | MIT OR Apache-2.0 |
| [keyring-core](https://github.com/open-source-cooperative/keyring-core.git) | 1.0.0 | MIT OR Apache-2.0 |
| [khronos_api](https://github.com/brendanzab/gl-rs/) | 3.1.0 | Apache-2.0 |
| [kqueue](https://gitlab.com/rust-kqueue/rust-kqueue) | 1.2.1 | MIT |
| [kqueue-sys](https://gitlab.com/rust-kqueue/rust-kqueue-sys) | 1.1.2 | MIT |
| [ksni](https://github.com/iovxw/ksni) | 0.3.6 | Unlicense |
| [kurbo](https://github.com/linebender/kurbo) | 0.11.3 | Apache-2.0 OR MIT |
| [kurbo](https://github.com/linebender/kurbo) | 0.13.1 | Apache-2.0 OR MIT |
| [lazy_static](https://github.com/rust-lang-nursery/lazy-static.rs) | 1.5.0 | MIT OR Apache-2.0 |
| [libbz2-rs-sys](https://github.com/trifectatechfoundation/libbzip2-rs) | 0.2.5 | bzip2-1.0.6 |
| [libc](https://github.com/rust-lang/libc) | 0.2.189 | MIT OR Apache-2.0 |
| [libloading](https://github.com/nagisa/rust_libloading/) | 0.8.9 | ISC |
| [libm](https://github.com/rust-lang/compiler-builtins) | 0.2.16 | MIT |
| [libredox](https://gitlab.redox-os.org/redox-os/libredox.git) | 0.1.20 | MIT |
| [linebender_resource_handle](https://github.com/linebender/raw_resource_handle) | 0.1.1 | Apache-2.0 OR MIT |
| [linereader](https://github.com/Freaky/rust-linereader.git) | 0.4.0 | MIT |
| [linux-raw-sys](https://github.com/sunfishcode/linux-raw-sys) | 0.12.1 | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT |
| [linux-raw-sys](https://github.com/sunfishcode/linux-raw-sys) | 0.4.15 | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT |
| [litemap](https://github.com/unicode-org/icu4x) | 0.8.3 | Unicode-3.0 |
| [litrs](https://github.com/LukasKalbertodt/litrs) | 1.0.0 | MIT OR Apache-2.0 |
| [lock_api](https://github.com/Amanieu/parking_lot) | 0.4.14 | MIT OR Apache-2.0 |
| [log](https://github.com/rust-lang/log) | 0.4.34 | MIT OR Apache-2.0 |
| [mach2](https://github.com/JohnTitor/mach2) | 0.6.0 | BSD-2-Clause OR MIT OR Apache-2.0 |
| [malloc_buf](https://github.com/SSheldon/malloc_buf) | 0.0.6 | MIT |
| [md-5](https://github.com/RustCrypto/hashes) | 0.10.6 | MIT OR Apache-2.0 |
| [memchr](https://github.com/BurntSushi/memchr) | 2.8.3 | Unlicense OR MIT |
| [memmap2](https://github.com/RazrFalcon/memmap2-rs) | 0.9.11 | MIT OR Apache-2.0 |
| [memoffset](https://github.com/Gilnaa/memoffset) | 0.9.1 | MIT |
| [mime](https://github.com/hyperium/mime) | 0.3.17 | MIT OR Apache-2.0 |
| [mime_guess2](https://github.com/ttys3/mime_guess2) | 2.3.1 | MIT |
| [miniz_oxide](https://github.com/Frommi/miniz_oxide/tree/master/miniz_oxide) | 0.8.9 | MIT OR Zlib OR Apache-2.0 |
| [mio](https://github.com/tokio-rs/mio) | 1.2.2 | MIT |
| [moxcms](https://github.com/awxkee/moxcms.git) | 0.8.1 | BSD-3-Clause OR Apache-2.0 |
| [mpris-server](https://github.com/SeaDve/mpris-server) | 0.10.0 | MPL-2.0 |
| [muda](https://github.com/tauri-apps/muda) | 0.20.0 | Apache-2.0 OR MIT |
| [ndk](https://github.com/rust-mobile/ndk) | 0.9.0 | MIT OR Apache-2.0 |
| [ndk-context](https://github.com/rust-windowing/android-ndk-rs) | 0.1.1 | MIT OR Apache-2.0 |
| [ndk-sys](https://github.com/rust-mobile/ndk) | 0.6.0+11769913 | MIT OR Apache-2.0 |
| [nohash-hasher](https://github.com/paritytech/nohash-hasher) | 0.2.0 | Apache-2.0 OR MIT |
| [nom](https://github.com/rust-bakery/nom) | 8.0.0 | MIT |
| [nom-language](https://github.com/rust-bakery/nom) | 0.1.0 | MIT |
| [notify](https://github.com/notify-rs/notify.git) | 8.2.0 | CC0-1.0 |
| [notify-types](https://github.com/notify-rs/notify.git) | 2.1.0 | MIT OR Apache-2.0 |
| [num](https://github.com/rust-num/num) | 0.4.3 | MIT OR Apache-2.0 |
| [num-bigint](https://github.com/rust-num/num-bigint) | 0.4.8 | MIT OR Apache-2.0 |
| [num-complex](https://github.com/rust-num/num-complex) | 0.4.6 | MIT OR Apache-2.0 |
| [num-derive](https://github.com/rust-num/num-derive) | 0.4.2 | MIT OR Apache-2.0 |
| [num-integer](https://github.com/rust-num/num-integer) | 0.1.47 | MIT OR Apache-2.0 |
| [num-iter](https://github.com/rust-num/num-iter) | 0.1.46 | MIT OR Apache-2.0 |
| [num-rational](https://github.com/rust-num/num-rational) | 0.4.2 | MIT OR Apache-2.0 |
| [num-traits](https://github.com/rust-num/num-traits) | 0.2.19 | MIT OR Apache-2.0 |
| [num_enum](https://github.com/illicitonion/num_enum) | 0.7.6 | BSD-3-Clause OR MIT OR Apache-2.0 |
| [num_enum_derive](https://github.com/illicitonion/num_enum) | 0.7.6 | BSD-3-Clause OR MIT OR Apache-2.0 |
| [objc](http://github.com/SSheldon/rust-objc) | 0.2.7 | MIT |
| [objc-sys](https://github.com/madsmtm/objc2) | 0.3.5 | MIT |
| [objc2](https://github.com/madsmtm/objc2) | 0.5.2 | MIT |
| [objc2](https://github.com/madsmtm/objc2) | 0.6.4 | MIT |
| [objc2-app-kit](https://github.com/madsmtm/objc2) | 0.2.2 | MIT |
| [objc2-app-kit](https://github.com/madsmtm/objc2) | 0.3.2 | Zlib OR Apache-2.0 OR MIT |
| [objc2-audio-toolbox](https://github.com/madsmtm/objc2) | 0.3.2 | Zlib OR Apache-2.0 OR MIT |
| [objc2-avf-audio](https://github.com/madsmtm/objc2) | 0.3.2 | Zlib OR Apache-2.0 OR MIT |
| [objc2-core-audio](https://github.com/madsmtm/objc2) | 0.3.2 | Zlib OR Apache-2.0 OR MIT |
| [objc2-core-audio-types](https://github.com/madsmtm/objc2) | 0.3.2 | Zlib OR Apache-2.0 OR MIT |
| [objc2-core-foundation](https://github.com/madsmtm/objc2) | 0.3.2 | Zlib OR Apache-2.0 OR MIT |
| [objc2-core-graphics](https://github.com/madsmtm/objc2) | 0.3.2 | Zlib OR Apache-2.0 OR MIT |
| [objc2-encode](https://github.com/madsmtm/objc2) | 4.1.0 | MIT |
| [objc2-exception-helper](https://github.com/madsmtm/objc2) | 0.1.1 | Zlib OR Apache-2.0 OR MIT |
| [objc2-foundation](https://github.com/madsmtm/objc2) | 0.2.2 | MIT |
| [objc2-foundation](https://github.com/madsmtm/objc2) | 0.3.2 | MIT |
| [objc2-ui-kit](https://github.com/madsmtm/objc2) | 0.2.2 | MIT |
| [objc2-ui-kit](https://github.com/madsmtm/objc2) | 0.3.2 | Zlib OR Apache-2.0 OR MIT |
| [once_cell](https://github.com/matklad/once_cell) | 1.21.4 | MIT OR Apache-2.0 |
| [once_cell_polyfill](https://github.com/polyfill-rs/once_cell_polyfill) | 1.70.2 | MIT OR Apache-2.0 |
| [open](https://github.com/Byron/open-rs) | 5.4.2 | MIT |
| [openh264](https://github.com/ralfbiedert/openh264-rs) | 0.9.8 | BSD-2-Clause |
| [openh264-sys2](https://github.com/ralfbiedert/openh264-rs) | 0.9.8 | BSD-2-Clause |
| [openssl-probe](https://github.com/rustls/openssl-probe) | 0.2.1 | MIT OR Apache-2.0 |
| [option-ext](https://github.com/soc/option-ext.git) | 0.2.0 | MPL-2.0 |
| [orbclient](https://gitlab.redox-os.org/redox-os/orbclient) | 0.3.55 | MIT |
| [ordered-stream](https://github.com/danieldg/ordered-stream) | 0.2.0 | MIT OR Apache-2.0 |
| [parking](https://github.com/smol-rs/parking) | 2.2.1 | Apache-2.0 OR MIT |
| [parking_lot](https://github.com/Amanieu/parking_lot) | 0.12.5 | MIT OR Apache-2.0 |
| [parking_lot_core](https://github.com/Amanieu/parking_lot) | 0.9.12 | MIT OR Apache-2.0 |
| [pastey](https://github.com/as1100k/pastey) | 0.2.3 | MIT OR Apache-2.0 |
| [peniko](https://github.com/linebender/peniko) | 0.6.1 | Apache-2.0 OR MIT |
| [percent-encoding](https://github.com/servo/rust-url/) | 2.3.2 | MIT OR Apache-2.0 |
| [phf](https://github.com/rust-phf/rust-phf) | 0.11.3 | MIT |
| [phf](https://github.com/rust-phf/rust-phf) | 0.13.1 | MIT |
| [phf_generator](https://github.com/rust-phf/rust-phf) | 0.13.1 | MIT |
| [phf_macros](https://github.com/rust-phf/rust-phf) | 0.13.1 | MIT |
| [phf_shared](https://github.com/rust-phf/rust-phf) | 0.11.3 | MIT |
| [phf_shared](https://github.com/rust-phf/rust-phf) | 0.13.1 | MIT |
| [pico-args](https://github.com/RazrFalcon/pico-args) | 0.5.0 | MIT |
| [pin-project](https://github.com/taiki-e/pin-project) | 1.1.13 | Apache-2.0 OR MIT |
| [pin-project-internal](https://github.com/taiki-e/pin-project) | 1.1.13 | Apache-2.0 OR MIT |
| [pin-project-lite](https://github.com/taiki-e/pin-project-lite) | 0.2.17 | Apache-2.0 OR MIT |
| [piper](https://github.com/smol-rs/piper) | 0.2.5 | MIT OR Apache-2.0 |
| [pkg-config](https://github.com/rust-lang/pkg-config-rs) | 0.3.34 | MIT OR Apache-2.0 |
| [plain](https://github.com/randomites/plain) | 0.2.3 | MIT/Apache-2.0 |
| [png](https://github.com/image-rs/image-png) | 0.17.16 | MIT OR Apache-2.0 |
| [png](https://github.com/image-rs/image-png) | 0.18.1 | MIT OR Apache-2.0 |
| [polib](https://github.com/brettdong/polib) | 0.3.0 | MIT |
| [polling](https://github.com/smol-rs/polling) | 3.11.0 | Apache-2.0 OR MIT |
| [pollster](https://github.com/zesterer/pollster) | 0.4.0 | Apache-2.0/MIT |
| [polycool](https://github.com/linebender/kurbo) | 0.4.0 | MIT OR Apache-2.0 |
| [portable-atomic](https://github.com/taiki-e/portable-atomic) | 1.15.0 | Apache-2.0 OR MIT |
| [portable-atomic-util](https://github.com/taiki-e/portable-atomic-util) | 0.2.7 | Apache-2.0 OR MIT |
| [potential_utf](https://github.com/unicode-org/icu4x) | 0.1.6 | Unicode-3.0 |
| [ppv-lite86](https://github.com/cryptocorrosion/cryptocorrosion) | 0.2.21 | MIT OR Apache-2.0 |
| [proc-macro-crate](https://github.com/bkchr/proc-macro-crate) | 3.5.0 | MIT OR Apache-2.0 |
| [proc-macro2](https://github.com/dtolnay/proc-macro2) | 1.0.107 | MIT OR Apache-2.0 |
| [profiling](https://github.com/aclysma/profiling) | 1.0.18 | MIT OR Apache-2.0 |
| [pxfm](https://github.com/awxkee/pxfm) | 0.1.30 | BSD-3-Clause OR Apache-2.0 |
| [quick-error](http://github.com/tailhook/quick-error) | 2.0.1 | MIT/Apache-2.0 |
| [quick-xml](https://github.com/tafia/quick-xml) | 0.41.0 | MIT |
| [quote](https://github.com/dtolnay/quote) | 1.0.47 | MIT OR Apache-2.0 |
| [r-efi](https://github.com/r-efi/r-efi) | 5.3.0 | MIT OR Apache-2.0 OR LGPL-2.1-or-later |
| [r-efi](https://github.com/r-efi/r-efi) | 6.0.0 | MIT OR Apache-2.0 OR LGPL-2.1-or-later |
| [rand](https://github.com/rust-random/rand) | 0.9.5 | MIT OR Apache-2.0 |
| [rand_chacha](https://github.com/rust-random/rand) | 0.9.0 | MIT OR Apache-2.0 |
| [rand_core](https://github.com/rust-random/rand) | 0.9.5 | MIT OR Apache-2.0 |
| [raw-window-handle](https://github.com/rust-windowing/raw-window-handle) | 0.6.2 | MIT OR Apache-2.0 OR Zlib |
| [read-fonts](https://github.com/googlefonts/fontations) | 0.41.0 | MIT OR Apache-2.0 |
| [redox_syscall](https://gitlab.redox-os.org/redox-os/syscall) | 0.4.1 | MIT |
| [redox_syscall](https://gitlab.redox-os.org/redox-os/syscall) | 0.5.18 | MIT |
| [redox_syscall](https://gitlab.redox-os.org/redox-os/syscall) | 0.9.3 | MIT |
| [redox_users](https://gitlab.redox-os.org/redox-os/users) | 0.5.2 | MIT |
| [relative-path](https://github.com/udoprog/relative-path) | 2.0.2 | MIT OR Apache-2.0 |
| [reqwest](https://github.com/seanmonstar/reqwest) | 0.12.28 | MIT OR Apache-2.0 |
| [resvg](https://github.com/linebender/resvg) | 0.45.1 | Apache-2.0 OR MIT |
| [rfd](https://github.com/PolyMeilex/rfd) | 0.17.2 | MIT |
| [rgb](https://github.com/kornelski/rust-rgb) | 0.8.53 | MIT |
| [ring](https://github.com/briansmith/ring) | 0.17.14 | Apache-2.0 AND ISC |
| [rodio](https://github.com/RustAudio/rodio) | 0.21.1 | MIT OR Apache-2.0 |
| [ron](https://github.com/ron-rs/ron) | 0.12.2 | MIT OR Apache-2.0 |
| [roxmltree](https://github.com/RazrFalcon/roxmltree) | 0.20.0 | MIT OR Apache-2.0 |
| [rquickjs](https://github.com/DelSkayn/rquickjs.git) | 0.14.0 | MIT |
| [rquickjs-core](https://github.com/DelSkayn/rquickjs.git) | 0.14.0 | MIT |
| [rquickjs-sys](https://github.com/DelSkayn/rquickjs.git) | 0.14.0 | MIT |
| [rustc_version](https://github.com/djc/rustc-version-rs) | 0.4.1 | MIT OR Apache-2.0 |
| [rustix](https://github.com/bytecodealliance/rustix) | 0.38.44 | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT |
| [rustix](https://github.com/bytecodealliance/rustix) | 1.1.4 | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT |
| [rustls](https://github.com/rustls/rustls) | 0.23.45 | Apache-2.0 OR ISC OR MIT |
| [rustls-native-certs](https://github.com/rustls/rustls-native-certs) | 0.8.4 | Apache-2.0 OR ISC OR MIT |
| [rustls-pki-types](https://github.com/rustls/pki-types) | 1.15.1 | MIT OR Apache-2.0 |
| [rustls-webpki](https://github.com/rustls/webpki) | 0.103.15 | ISC |
| [rustversion](https://github.com/dtolnay/rustversion) | 1.0.23 | MIT OR Apache-2.0 |
| [rusty_aac](https://github.com/Remade-With-Rust/remade_ffmpeg_rs) | 1.1.0 | Apache-2.0 |
| [rusty_vp9](https://github.com/Remade-With-Rust/remade_ffmpeg_rs) | 0.1.2 | Apache-2.0 |
| [ryu](https://github.com/dtolnay/ryu) | 1.0.23 | Apache-2.0 OR BSL-1.0 |
| [safe_arch](https://github.com/Lokathor/safe_arch) | 1.2.0 | Zlib OR Apache-2.0 OR MIT |
| [same-file](https://github.com/BurntSushi/same-file) | 1.0.6 | Unlicense/MIT |
| [schannel](https://github.com/steffengy/schannel-rs) | 0.1.29 | MIT |
| [scoped-tls](https://github.com/alexcrichton/scoped-tls) | 1.0.1 | MIT/Apache-2.0 |
| [scopeguard](https://github.com/bluss/scopeguard) | 1.2.0 | MIT OR Apache-2.0 |
| [secret-service](https://github.com/hwchen/secret-service-rs.git) | 5.2.0 | MIT OR Apache-2.0 |
| [security-framework](https://github.com/kornelski/rust-security-framework) | 3.7.0 | MIT OR Apache-2.0 |
| [security-framework-sys](https://github.com/kornelski/rust-security-framework) | 2.17.0 | MIT OR Apache-2.0 |
| [self_cell](https://github.com/Voultapher/self_cell) | 1.3.0 | Apache-2.0 OR GPL-2.0-only |
| [semver](https://github.com/dtolnay/semver) | 1.0.28 | MIT OR Apache-2.0 |
| [serde](https://github.com/serde-rs/serde) | 1.0.229 | MIT OR Apache-2.0 |
| [serde_core](https://github.com/serde-rs/serde) | 1.0.229 | MIT OR Apache-2.0 |
| [serde_derive](https://github.com/serde-rs/serde) | 1.0.229 | MIT OR Apache-2.0 |
| [serde_json](https://github.com/serde-rs/json) | 1.0.151 | MIT OR Apache-2.0 |
| [serde_repr](https://github.com/dtolnay/serde-repr) | 0.1.21 | MIT OR Apache-2.0 |
| [serde_spanned](https://github.com/toml-rs/toml) | 1.1.1 | MIT OR Apache-2.0 |
| [serde_urlencoded](https://github.com/nox/serde_urlencoded) | 0.7.1 | MIT/Apache-2.0 |
| [sha1](https://github.com/RustCrypto/hashes) | 0.10.7 | MIT OR Apache-2.0 |
| [sha2](https://github.com/RustCrypto/hashes) | 0.10.9 | MIT OR Apache-2.0 |
| [sha2](https://github.com/RustCrypto/hashes) | 0.11.0 | MIT OR Apache-2.0 |
| [shlex](https://github.com/comex/rust-shlex) | 2.0.1 | MIT OR Apache-2.0 |
| [signal-hook-registry](https://github.com/vorner/signal-hook) | 1.4.8 | MIT OR Apache-2.0 |
| [simd-adler32](https://github.com/mcountryman/simd-adler32) | 0.3.10 | MIT |
| [simd_cesu8](https://github.com/seancroach/simd_cesu8) | 1.2.0 | Apache-2.0 OR MIT |
| [simdutf8](https://github.com/rusticstuff/simdutf8) | 0.1.5 | MIT OR Apache-2.0 |
| [simplecss](https://github.com/linebender/simplecss) | 0.2.2 | Apache-2.0 OR MIT |
| [siphasher](https://github.com/jedisct1/rust-siphash) | 1.0.3 | MIT/Apache-2.0 |
| [skrifa](https://github.com/googlefonts/fontations) | 0.44.0 | MIT OR Apache-2.0 |
| [slab](https://github.com/tokio-rs/slab) | 0.4.12 | MIT |
| [slotmap](https://github.com/orlp/slotmap) | 1.1.1 | Zlib |
| [smallvec](https://github.com/servo/rust-smallvec) | 1.15.2 | MIT OR Apache-2.0 |
| [smithay-client-toolkit](https://github.com/smithay/client-toolkit) | 0.19.2 | MIT |
| [smithay-client-toolkit](https://github.com/smithay/client-toolkit) | 0.20.0 | MIT |
| [smithay-clipboard](https://github.com/smithay/smithay-clipboard) | 0.7.3 | MIT |
| [smol_str](https://github.com/rust-analyzer/smol_str) | 0.2.2 | MIT OR Apache-2.0 |
| [socket2](https://github.com/rust-lang/socket2) | 0.6.5 | MIT OR Apache-2.0 |
| [souvlaki](https://github.com/Sinono3/souvlaki) | 0.8.3 | MIT |
| [spin](https://github.com/mvdnes/spin-rs.git) | 0.9.9 | MIT |
| [stable_deref_trait](https://github.com/storyyeller/stable_deref_trait) | 1.2.1 | MIT OR Apache-2.0 |
| [static_assertions](https://github.com/nvzqz/static-assertions-rs) | 1.1.0 | MIT OR Apache-2.0 |
| [strict-num](https://github.com/RazrFalcon/strict-num) | 0.1.1 | MIT |
| [strsim](https://github.com/rapidfuzz/strsim-rs) | 0.11.1 | MIT |
| [subtle](https://github.com/dalek-cryptography/subtle) | 2.6.1 | BSD-3-Clause |
| [svgtypes](https://github.com/linebender/svgtypes) | 0.15.3 | Apache-2.0 OR MIT |
| [symphonia](https://github.com/pdeljanov/Symphonia) | 0.5.5 | MPL-2.0 |
| [symphonia-bundle-flac](https://github.com/pdeljanov/Symphonia) | 0.5.5 | MPL-2.0 |
| [symphonia-codec-aac](https://github.com/pdeljanov/Symphonia) | 0.5.5 | MPL-2.0 |
| [symphonia-codec-alac](https://github.com/pdeljanov/Symphonia) | 0.5.5 | MPL-2.0 |
| [symphonia-codec-pcm](https://github.com/pdeljanov/Symphonia) | 0.5.5 | MPL-2.0 |
| [symphonia-core](https://github.com/pdeljanov/Symphonia) | 0.5.5 | MPL-2.0 |
| [symphonia-format-isomp4](https://github.com/pdeljanov/Symphonia) | 0.5.5 | MPL-2.0 |
| [symphonia-format-riff](https://github.com/pdeljanov/Symphonia) | 0.5.5 | MPL-2.0 |
| [symphonia-metadata](https://github.com/pdeljanov/Symphonia) | 0.5.5 | MPL-2.0 |
| [symphonia-utils-xiph](https://github.com/pdeljanov/Symphonia) | 0.5.5 | MPL-2.0 |
| [syn](https://github.com/dtolnay/syn) | 2.0.119 | MIT OR Apache-2.0 |
| [syn](https://github.com/dtolnay/syn) | 3.0.4 | MIT OR Apache-2.0 |
| [sync_wrapper](https://github.com/Actyx/sync_wrapper) | 1.0.2 | Apache-2.0 |
| [synstructure](https://github.com/mystor/synstructure) | 0.13.2 | MIT |
| [sys-locale](https://github.com/1Password/sys-locale) | 0.3.2 | MIT OR Apache-2.0 |
| [system-configuration](https://github.com/mullvad/system-configuration-rs) | 0.7.0 | MIT OR Apache-2.0 |
| [system-configuration-sys](https://github.com/mullvad/system-configuration-rs) | 0.6.0 | MIT OR Apache-2.0 |
| [tempfile](https://github.com/Stebalien/tempfile) | 3.27.0 | MIT OR Apache-2.0 |
| [thiserror](https://github.com/dtolnay/thiserror) | 1.0.69 | MIT OR Apache-2.0 |
| [thiserror](https://github.com/dtolnay/thiserror) | 2.0.20 | MIT OR Apache-2.0 |
| [thiserror-impl](https://github.com/dtolnay/thiserror) | 1.0.69 | MIT OR Apache-2.0 |
| [thiserror-impl](https://github.com/dtolnay/thiserror) | 2.0.20 | MIT OR Apache-2.0 |
| [tiff](https://github.com/image-rs/image-tiff) | 0.11.3 | MIT |
| [tiny-skia](https://github.com/RazrFalcon/tiny-skia) | 0.11.4 | BSD-3-Clause |
| [tiny-skia-path](https://github.com/RazrFalcon/tiny-skia/tree/master/path) | 0.11.4 | BSD-3-Clause |
| [tinystr](https://github.com/unicode-org/icu4x) | 0.8.4 | Unicode-3.0 |
| [tokio](https://github.com/tokio-rs/tokio) | 1.53.1 | MIT |
| [tokio-macros](https://github.com/tokio-rs/tokio) | 2.7.2 | MIT |
| [tokio-rustls](https://github.com/rustls/tokio-rustls) | 0.26.4 | MIT OR Apache-2.0 |
| [tokio-util](https://github.com/tokio-rs/tokio) | 0.7.19 | MIT |
| [toml](https://github.com/toml-rs/toml) | 1.1.4+spec-1.1.0 | MIT OR Apache-2.0 |
| [toml_datetime](https://github.com/toml-rs/toml) | 1.1.1+spec-1.1.0 | MIT OR Apache-2.0 |
| [toml_edit](https://github.com/toml-rs/toml) | 0.25.13+spec-1.1.0 | MIT OR Apache-2.0 |
| [toml_parser](https://github.com/toml-rs/toml) | 1.1.3+spec-1.1.0 | MIT OR Apache-2.0 |
| [tower](https://github.com/tower-rs/tower) | 0.5.3 | MIT |
| [tower-http](https://github.com/tower-rs/tower-http) | 0.6.11 | MIT |
| [tower-layer](https://github.com/tower-rs/tower) | 0.3.3 | MIT |
| [tower-service](https://github.com/tower-rs/tower) | 0.3.3 | MIT |
| [tr](https://github.com/woboq/tr) | 0.1.11 | MIT |
| [tracing](https://github.com/tokio-rs/tracing) | 0.1.44 | MIT |
| [tracing-attributes](https://github.com/tokio-rs/tracing) | 0.1.31 | MIT |
| [tracing-core](https://github.com/tokio-rs/tracing) | 0.1.36 | MIT |
| [trait-variant](https://github.com/rust-lang/impl-trait-utils) | 0.1.3 | MIT OR Apache-2.0 |
| [tray-icon](https://github.com/tauri-apps/tray-icon) | 0.25.1 | MIT OR Apache-2.0 |
| [try-lock](https://github.com/seanmonstar/try-lock) | 0.2.5 | MIT |
| [typeid](https://github.com/dtolnay/typeid) | 1.0.3 | MIT OR Apache-2.0 |
| [typenum](https://github.com/paholg/typenum) | 1.20.1 | MIT OR Apache-2.0 |
| [uds_windows](https://github.com/haraldh/rust_uds_windows) | 1.2.1 | MIT |
| [unicase](https://github.com/seanmonstar/unicase) | 2.9.0 | MIT OR Apache-2.0 |
| [unicode-bidi](https://github.com/servo/unicode-bidi) | 0.3.18 | MIT OR Apache-2.0 |
| [unicode-general-category](https://github.com/yeslogic/unicode-general-category) | 1.1.0 | Apache-2.0 |
| [unicode-ident](https://github.com/dtolnay/unicode-ident) | 1.0.24 | (MIT OR Apache-2.0) AND Unicode-3.0 |
| [unicode-segmentation](https://github.com/unicode-rs/unicode-segmentation) | 1.13.3 | MIT OR Apache-2.0 |
| [untrusted](https://github.com/briansmith/untrusted) | 0.9.0 | ISC |
| [url](https://github.com/servo/rust-url) | 2.5.8 | MIT OR Apache-2.0 |
| [urlencoding](https://github.com/kornelski/rust_urlencoding) | 2.1.3 | MIT |
| [usvg](https://github.com/linebender/resvg) | 0.45.1 | Apache-2.0 OR MIT |
| [utf8_iter](https://github.com/hsivonen/utf8_iter) | 1.0.4 | Apache-2.0 OR MIT |
| [utf8parse](https://github.com/alacritty/vte) | 0.2.2 | Apache-2.0 OR MIT |
| [uuid](https://github.com/uuid-rs/uuid) | 1.26.0 | Apache-2.0 OR MIT |
| [vello_common](https://github.com/linebender/vello) | 0.1.0 | Apache-2.0 OR MIT |
| [vello_cpu](https://github.com/linebender/vello) | 0.1.0 | Apache-2.0 OR MIT |
| [version_check](https://github.com/SergioBenitez/version_check) | 0.9.5 | MIT/Apache-2.0 |
| [walkdir](https://github.com/BurntSushi/walkdir) | 2.5.0 | Unlicense/MIT |
| [want](https://github.com/seanmonstar/want) | 0.3.1 | MIT |
| [wasi](https://github.com/bytecodealliance/wasi) | 0.11.1+wasi-snapshot-preview1 | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT |
| [wasip2](https://github.com/bytecodealliance/wasi-rs) | 1.0.4+wasi-0.2.12 | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT |
| [wasm-bindgen](https://github.com/wasm-bindgen/wasm-bindgen) | 0.2.127 | MIT OR Apache-2.0 |
| [wasm-bindgen-futures](https://github.com/wasm-bindgen/wasm-bindgen/tree/master/crates/futures) | 0.4.77 | MIT OR Apache-2.0 |
| [wasm-bindgen-macro](https://github.com/wasm-bindgen/wasm-bindgen/tree/master/crates/macro) | 0.2.127 | MIT OR Apache-2.0 |
| [wasm-bindgen-macro-support](https://github.com/wasm-bindgen/wasm-bindgen/tree/master/crates/macro-support) | 0.2.127 | MIT OR Apache-2.0 |
| [wasm-bindgen-shared](https://github.com/wasm-bindgen/wasm-bindgen/tree/master/crates/shared) | 0.2.127 | MIT OR Apache-2.0 |
| [wasmi](https://github.com/wasmi-labs/wasmi) | 2.0.0 | MIT/Apache-2.0 |
| [wasmi_collections](https://github.com/wasmi-labs/wasmi) | 2.0.0 | MIT/Apache-2.0 |
| [wasmi_core](https://github.com/wasmi-labs/wasmi) | 2.0.0 | MIT/Apache-2.0 |
| [wasmi_ir](https://github.com/wasmi-labs/wasmi) | 2.0.0 | MIT/Apache-2.0 |
| [wasmparser](https://github.com/bytecodealliance/wasm-tools/tree/main/crates/wasmparser) | 0.228.0 | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT |
| [wayland-backend](https://github.com/smithay/wayland-rs) | 0.3.17 | MIT |
| [wayland-client](https://github.com/smithay/wayland-rs) | 0.31.15 | MIT |
| [wayland-csd-frame](https://github.com/rust-windowing/wayland-csd-frame) | 0.3.0 | MIT |
| [wayland-cursor](https://github.com/smithay/wayland-rs) | 0.31.14 | MIT |
| [wayland-protocols](https://github.com/smithay/wayland-rs) | 0.32.13 | MIT |
| [wayland-protocols-experimental](https://github.com/smithay/wayland-rs) | 20250721.0.1 | MIT |
| [wayland-protocols-misc](https://github.com/smithay/wayland-rs) | 0.3.12 | MIT |
| [wayland-protocols-plasma](https://github.com/smithay/wayland-rs) | 0.3.12 | MIT |
| [wayland-protocols-wlr](https://github.com/smithay/wayland-rs) | 0.3.12 | MIT |
| [wayland-scanner](https://github.com/smithay/wayland-rs) | 0.31.11 | MIT |
| [wayland-sys](https://github.com/smithay/wayland-rs) | 0.31.11 | MIT |
| [web-sys](https://github.com/wasm-bindgen/wasm-bindgen/tree/master/crates/web-sys) | 0.3.104 | MIT OR Apache-2.0 |
| [web-time](https://github.com/daxpedda/web-time) | 1.1.0 | MIT OR Apache-2.0 |
| [webbrowser](https://github.com/amodm/webbrowser-rs) | 1.2.4 | MIT OR Apache-2.0 |
| [weezl](https://github.com/image-rs/weezl) | 0.1.12 | MIT OR Apache-2.0 |
| [wide](https://github.com/Lokathor/wide) | 1.7.1 | Zlib OR Apache-2.0 OR MIT |
| [winapi-util](https://github.com/BurntSushi/winapi-util) | 0.1.11 | Unlicense OR MIT |
| [windows](https://github.com/microsoft/windows-rs) | 0.44.0 | MIT OR Apache-2.0 |
| [windows](https://github.com/microsoft/windows-rs) | 0.62.2 | MIT OR Apache-2.0 |
| [windows-collections](https://github.com/microsoft/windows-rs) | 0.3.2 | MIT OR Apache-2.0 |
| [windows-core](https://github.com/microsoft/windows-rs) | 0.62.2 | MIT OR Apache-2.0 |
| [windows-future](https://github.com/microsoft/windows-rs) | 0.3.2 | MIT OR Apache-2.0 |
| [windows-implement](https://github.com/microsoft/windows-rs) | 0.60.2 | MIT OR Apache-2.0 |
| [windows-interface](https://github.com/microsoft/windows-rs) | 0.59.3 | MIT OR Apache-2.0 |
| [windows-link](https://github.com/microsoft/windows-rs) | 0.2.1 | MIT OR Apache-2.0 |
| [windows-native-keyring-store](https://github.com/open-source-cooperative/windows-native-keyring-store.git) | 1.1.0 | MIT OR Apache-2.0 |
| [windows-numerics](https://github.com/microsoft/windows-rs) | 0.3.1 | MIT OR Apache-2.0 |
| [windows-registry](https://github.com/microsoft/windows-rs) | 0.6.1 | MIT OR Apache-2.0 |
| [windows-result](https://github.com/microsoft/windows-rs) | 0.4.1 | MIT OR Apache-2.0 |
| [windows-strings](https://github.com/microsoft/windows-rs) | 0.5.1 | MIT OR Apache-2.0 |
| [windows-sys](https://github.com/microsoft/windows-rs) | 0.52.0 | MIT OR Apache-2.0 |
| [windows-sys](https://github.com/microsoft/windows-rs) | 0.59.0 | MIT OR Apache-2.0 |
| [windows-sys](https://github.com/microsoft/windows-rs) | 0.60.2 | MIT OR Apache-2.0 |
| [windows-sys](https://github.com/microsoft/windows-rs) | 0.61.2 | MIT OR Apache-2.0 |
| [windows-targets](https://github.com/microsoft/windows-rs) | 0.42.2 | MIT OR Apache-2.0 |
| [windows-targets](https://github.com/microsoft/windows-rs) | 0.52.6 | MIT OR Apache-2.0 |
| [windows-targets](https://github.com/microsoft/windows-rs) | 0.53.5 | MIT OR Apache-2.0 |
| [windows-threading](https://github.com/microsoft/windows-rs) | 0.2.1 | MIT OR Apache-2.0 |
| [windows_aarch64_gnullvm](https://github.com/microsoft/windows-rs) | 0.42.2 | MIT OR Apache-2.0 |
| [windows_aarch64_gnullvm](https://github.com/microsoft/windows-rs) | 0.52.6 | MIT OR Apache-2.0 |
| [windows_aarch64_gnullvm](https://github.com/microsoft/windows-rs) | 0.53.1 | MIT OR Apache-2.0 |
| [windows_aarch64_msvc](https://github.com/microsoft/windows-rs) | 0.42.2 | MIT OR Apache-2.0 |
| [windows_aarch64_msvc](https://github.com/microsoft/windows-rs) | 0.52.6 | MIT OR Apache-2.0 |
| [windows_aarch64_msvc](https://github.com/microsoft/windows-rs) | 0.53.1 | MIT OR Apache-2.0 |
| [windows_i686_gnu](https://github.com/microsoft/windows-rs) | 0.42.2 | MIT OR Apache-2.0 |
| [windows_i686_gnu](https://github.com/microsoft/windows-rs) | 0.52.6 | MIT OR Apache-2.0 |
| [windows_i686_gnu](https://github.com/microsoft/windows-rs) | 0.53.1 | MIT OR Apache-2.0 |
| [windows_i686_gnullvm](https://github.com/microsoft/windows-rs) | 0.52.6 | MIT OR Apache-2.0 |
| [windows_i686_gnullvm](https://github.com/microsoft/windows-rs) | 0.53.1 | MIT OR Apache-2.0 |
| [windows_i686_msvc](https://github.com/microsoft/windows-rs) | 0.42.2 | MIT OR Apache-2.0 |
| [windows_i686_msvc](https://github.com/microsoft/windows-rs) | 0.52.6 | MIT OR Apache-2.0 |
| [windows_i686_msvc](https://github.com/microsoft/windows-rs) | 0.53.1 | MIT OR Apache-2.0 |
| [windows_x86_64_gnu](https://github.com/microsoft/windows-rs) | 0.42.2 | MIT OR Apache-2.0 |
| [windows_x86_64_gnu](https://github.com/microsoft/windows-rs) | 0.52.6 | MIT OR Apache-2.0 |
| [windows_x86_64_gnu](https://github.com/microsoft/windows-rs) | 0.53.1 | MIT OR Apache-2.0 |
| [windows_x86_64_gnullvm](https://github.com/microsoft/windows-rs) | 0.42.2 | MIT OR Apache-2.0 |
| [windows_x86_64_gnullvm](https://github.com/microsoft/windows-rs) | 0.52.6 | MIT OR Apache-2.0 |
| [windows_x86_64_gnullvm](https://github.com/microsoft/windows-rs) | 0.53.1 | MIT OR Apache-2.0 |
| [windows_x86_64_msvc](https://github.com/microsoft/windows-rs) | 0.42.2 | MIT OR Apache-2.0 |
| [windows_x86_64_msvc](https://github.com/microsoft/windows-rs) | 0.52.6 | MIT OR Apache-2.0 |
| [windows_x86_64_msvc](https://github.com/microsoft/windows-rs) | 0.53.1 | MIT OR Apache-2.0 |
| [winit](https://github.com/rust-windowing/winit) | 0.30.13 | Apache-2.0 |
| [winnow](https://github.com/winnow-rs/winnow) | 1.0.4 | MIT |
| [winresource](https://github.com/BenjaminRi/winresource) | 0.1.31 | MIT |
| [wit-bindgen](https://github.com/bytecodealliance/wit-bindgen) | 0.57.1 | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT |
| [writeable](https://github.com/unicode-org/icu4x) | 0.6.4 | Unicode-3.0 |
| [x11-dl](https://github.com/AltF02/x11-rs.git) | 2.21.0 | MIT |
| [x11rb](https://github.com/psychon/x11rb) | 0.13.2 | MIT OR Apache-2.0 |
| [x11rb-protocol](https://github.com/psychon/x11rb) | 0.13.2 | MIT OR Apache-2.0 |
| [xcursor](https://github.com/esposm03/xcursor-rs) | 0.3.11 | MIT |
| [xkbcommon-dl](https://github.com/rust-windowing/xkbcommon-dl) | 0.4.2 | MIT |
| [xkeysym](https://github.com/notgull/xkeysym) | 0.2.1 | MIT OR Apache-2.0 OR Zlib |
| [xml-rs](https://github.com/kornelski/xml-rs) | 0.8.29 | MIT |
| [xmlwriter](https://github.com/RazrFalcon/xmlwriter) | 0.1.0 | MIT |
| [yoke](https://github.com/unicode-org/icu4x) | 0.8.3 | Unicode-3.0 |
| [yoke-derive](https://github.com/unicode-org/icu4x) | 0.8.2 | Unicode-3.0 |
| [zbus](https://github.com/z-galaxy/zbus/) | 5.19.0 | MIT |
| [zbus-lockstep](https://github.com/luukvanderduim/zbus-lockstep) | 0.5.2 | MIT |
| [zbus-lockstep-macros](https://github.com/luukvanderduim/zbus-lockstep) | 0.5.2 | MIT |
| [zbus-secret-service-keyring-store](https://github.com/open-source-cooperative/zbus-secret-service-keyring-store.git) | 1.0.1 | MIT OR Apache-2.0 |
| [zbus_macros](https://github.com/z-galaxy/zbus/) | 5.19.0 | MIT |
| [zbus_names](https://github.com/z-galaxy/zbus/) | 4.3.4 | MIT |
| [zbus_xml](https://github.com/z-galaxy/zbus/) | 5.2.1 | MIT |
| [zcheapstr](https://github.com/z-galaxy/zcheapstr/) | 1.1.0 | MIT |
| [zerocopy](https://github.com/google/zerocopy) | 0.8.56 | BSD-2-Clause OR Apache-2.0 OR MIT |
| [zerocopy-derive](https://github.com/google/zerocopy) | 0.8.56 | BSD-2-Clause OR Apache-2.0 OR MIT |
| [zerofrom](https://github.com/unicode-org/icu4x) | 0.1.8 | Unicode-3.0 |
| [zerofrom-derive](https://github.com/unicode-org/icu4x) | 0.1.7 | Unicode-3.0 |
| [zeroize](https://github.com/RustCrypto/utils) | 1.9.0 | Apache-2.0 OR MIT |
| [zerotrie](https://github.com/unicode-org/icu4x) | 0.2.5 | Unicode-3.0 |
| [zerovec](https://github.com/unicode-org/icu4x) | 0.11.8 | Unicode-3.0 |
| [zerovec-derive](https://github.com/unicode-org/icu4x) | 0.11.6 | Unicode-3.0 |
| [zmij](https://github.com/dtolnay/zmij) | 1.0.23 | MIT |
| [zune-core](https://github.com/etemesi254/zune-image) | 0.5.3 | MIT OR Apache-2.0 OR Zlib |
| [zune-jpeg](https://github.com/etemesi254/zune-image/tree/dev/crates/zune-jpeg) | 0.5.15 | MIT OR Apache-2.0 OR Zlib |
| [zvariant](https://github.com/z-galaxy/zbus/) | 5.15.0 | MIT |
| [zvariant_derive](https://github.com/z-galaxy/zbus/) | 5.15.0 | MIT |
| [zvariant_utils](https://github.com/z-galaxy/zbus/) | 4.2.0 | MIT |
