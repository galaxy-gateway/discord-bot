# Persona Portrait Assets

SVG source files for persona avatars. These need to be converted to PNG for Discord compatibility.

## Files

| Persona | SVG | Description |
|---------|-----|-------------|
| obi | `obi.svg` | Hooded Jedi with lightsaber hint |
| muppet | `muppet.svg` | Green fuzzy friend with googly eyes |
| chef | `chef.svg` | Chef with toque and mustache |
| teacher | `teacher.svg` | Friendly teacher with glasses and apple |
| analyst | `analyst.svg` | Chart/graph with magnifying glass |
| visionary | `visionary.svg` | Crystal ball with mystical eye |
| noir | `noir.svg` | Detective in fedora with shadows |
| zen | `zen.svg` | Meditating figure with enso circle |
| bard | `bard.svg` | Lute/lyre with musical notes |
| coach | `coach.svg` | Whistle and clipboard with play diagram |
| scientist | `scientist.svg` | Bubbling flask with atom symbol |
| gamer | `gamer.svg` | Game controller with RGB glow |

## Converting to PNG

Discord only supports PNG/JPG/GIF/WebP images. Convert SVGs to PNG before use.

### Option 1: Using ImageMagick (recommended)

```bash
# Install ImageMagick if needed
# Ubuntu/Debian: sudo apt install imagemagick
# macOS: brew install imagemagick

# Convert all SVGs to 128x128 PNGs
make convert-portraits

# Or manually:
for svg in assets/personas/*.svg; do
  convert -background none -resize 128x128 "$svg" "${svg%.svg}.png"
done
```

### Option 2: Using Inkscape

```bash
# Install Inkscape if needed
# Ubuntu/Debian: sudo apt install inkscape
# macOS: brew install inkscape

for svg in assets/personas/*.svg; do
  inkscape "$svg" -w 128 -h 128 -o "${svg%.svg}.png"
done
```

### Option 3: Using rsvg-convert

```bash
# Install librsvg if needed
# Ubuntu/Debian: sudo apt install librsvg2-bin
# macOS: brew install librsvg

for svg in assets/personas/*.svg; do
  rsvg-convert -w 128 -h 128 "$svg" -o "${svg%.svg}.png"
done
```

## Hosting for Discord

After converting to PNG, host the images somewhere accessible via URL:

1. **GitHub Raw URLs** (free, simple)
   - Push PNGs to repo
   - Use: `https://raw.githubusercontent.com/USER/REPO/main/assets/personas/obi.png`

2. **GitHub Pages** (free, custom domain option)
   - Enable Pages on the repo
   - Use: `https://USER.github.io/REPO/assets/personas/obi.png`

3. **CDN** (Cloudflare, AWS S3, etc.)
   - Upload PNGs to CDN
   - Configure CORS if needed

## Updating Portraits

1. Edit the SVG file
2. Run `make convert-portraits` to regenerate PNGs
3. Commit both SVG and PNG files
4. If using GitHub raw URLs, they update automatically

## Design Guidelines

- 100x100 viewBox for consistency
- Use persona's theme color as background
- Simple, recognizable iconography
- Good contrast at small sizes (Discord shows ~40x40px)
