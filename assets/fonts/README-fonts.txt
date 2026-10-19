Fonts bundled with Tidalfast

HankenGrotesk.ttf (licence: HankenGrotesk-OFL.txt)
  The interface face, variable (wght 100-900). It is the Google Fonts release
  with one change: the Greek, Greek Extended and Cyrillic entries of its
  character map (mu, pi, Delta, Omega, tonos marks, and the Ohm sign) are removed. The glyphs and every Latin, Vietnamese and extended
  Latin character are untouched. Why: Hanken has no real Greek or Cyrillic, but
  mapped a handful of Greek letters, so a Greek word came half from Hanken and
  half from a fallback face, in two weights. Without them the whole run falls
  through to Inter (below). To redo: open the original with fontTools and
  delete code points 0370-03FF, 1F00-1FFF, 0400-052F, 1C80-1C8F, 2DE0-2DFF,
  A640-A69F and 2126 from every cmap subtable.

Inter (licence: Inter-LICENSE.txt)
  Not stored here: fastframe-fonts already embeds InterVariable.ttf (Inter
  4.001), so Greek, Cyrillic and Vietnamese cost no extra binary size. It
  follows Hanken in every weight family at the same wght and at 0.95 scale
  (Inter is 9% wider). See src/fonts.rs. Other scripts use the installed
  system face at the family's weight.

NotoEmoji.ttf (licence: NotoEmoji-LICENSE.txt)
  Monochrome emoji.
