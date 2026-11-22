---
name: geo-magazine-slides
description: "Create stunning geographic magazine-style presentation decks (PPTX) with editorial-quality layouts, large hero imagery, data-driven charts, and luxury aesthetic. Use for location collections, luxury real estate showcases, travel destination decks, curated map-based presentations, or architecture portfolios with magazine-style grid overviews and section dividers."
---

# Geographic Magazine Presentation

Create high-end editorial presentation decks modeled after luxury geographic magazines. The style combines large-format photography placeholders, clean serif/sans-serif typography, a bold red-and-gray color system, and structured data visualizations.

## Reference Source

- **Type**: Uploaded artifact (PPTX)
- **Reference artifact type**: PPTX
- **Effective Reference File Type**: PPTX
- **Fonts extracted from reference**: Oranienbaum (serif headings), Liter (sans-serif body)

## Supported Outputs

- **PPTX** (default and primary)

## Default Output Selection

- If user explicitly requests a format, use that format
- Otherwise, default to **PPTX**

## Workflow

### 1. Gather Content Requirements

Collect from the user:
- **Topic**: What geographic collection is being presented (city, region, properties, landmarks)
- **Items**: The individual items/locations to feature (target ~20 items for full structure)
- **Data points**: Key metrics per item (prices, sizes, dates, ratings — varies by domain)
- **Visual assets**: Available photos or confirm use of dark placeholder rectangles
- **Narrative flow**: Preferred grouping by region, category, or ranking

### 2. Read the Style and Structure Contracts

Before creating any slides, read both reference files to internalize the visual language:

- **`references/style_contract.md`** — Full color palette (exact hex), typography system, layout rules with measurements, image/chart treatment, and slide-specific layout patterns
- **`references/structure_contract.md`** — Exact 15-slide sequence, section hierarchy, content block patterns, and mandatory elements checklist

### 3. Plan the Slide Deck

Map content to the required 15-slide sequence (see structure contract). For each slide:
- Determine slide type (cover, overview, section divider, detail, chart, theme, closing)
- Assign text content and hierarchy levels
- Plan image placeholder positions (dark rectangles, NEVER external image downloads)
- Prepare data for charts (horizontal bar chart on slide 5, vertical bar chart on slide 9)

### 4. Create the Presentation

Build the deck slide by slide following the style contract exactly.

#### CRITICAL: Color Palette (Non-Negotiable)

| Element | Color | Hex |
|---------|-------|-----|
| Slide background | Light warm gray | `#CCCCCC` or `#D3D3D3` |
| Primary accent | Red | `#D32F2F` |
| Headings / chart bars | Black | `#000000` |
| Body text | Dark gray | `#333333` |
| Image placeholders | Near-black range | `#1A1A1A` – `#353535` |
| Text on dark bars | White | `#FFFFFF` |
| Info card backgrounds | White | `#FFFFFF` |

**NEVER** use cream, beige, brown, copper, or gold tones. The background must be gray, and the accent must be the exact red `#D32F2F`.

#### Typography Rules

- **Headings**: Oranienbaum serif, ALL CAPS, black (`#000000`)
- **Body**: Liter sans-serif, sentence case, dark gray (`#333333`)
- **Emphasis**: Red (`#D32F2F`) for prices, locations, key statistics, bullet dots
- **Taglines**: Liter, ALL CAPS, red (`#D32F2F`)
- **Captions/Labels**: Liter, ALL CAPS, 9 pt
- **Red underline**: Short horizontal rule (~1" wide, 4–6 px, `#D32F2F`) beneath main titles

#### CRITICAL: Cover Slide Layout (Must Match Exactly)

- Left ~45%: Light-gray panel with:
  - Thin red vertical accent bar on left edge (~0.1" wide, ~4" tall)
  - Title: Oranienbaum, 52 pt, ALL CAPS, black
  - Tagline: Liter, 16 pt, ALL CAPS, red (`#D32F2F`)
- Right ~55%: Full-height B&W hero photograph
  - Black diagonal accent bar crossing lower portion (~1" wide)
  - Red diagonal accent bar parallel to black one (~0.6" wide)
- White rectangle at bottom-right corner
- NO footer text on cover

#### CRITICAL: Overview Grid (Slide 2)

- 5-column x 4-row grid showing ALL 20 items
- Each card: Dark placeholder rectangle (~3.06" x 0.97") + property name (bold, ALL CAPS) + location tag (red, "| LOCATION" format)
- Vary placeholder shades: `#1A1A1A`, `#222222`, `#252525`, `#282828`, `#2A2A2A`, `#2D2D2D`, `#303030`, `#333333`, `#353535`
- Title above grid: Oranienbaum, 38 pt, ALL CAPS

#### CRITICAL: Chart Slides (Slides 5 and 9 — Must Include Both)

**Slide 5 — Horizontal Bar Chart:**
- Left side: Horizontal bar chart with black bars, item names on Y-axis, values at bar ends
- Right side: 2x2 grid of white info cards (property name, price in red, brief specs)
- Below cards: Feature heading + 2-column red dot bullet list
- Bottom: Full-width black info bar with market data

**Slide 9 — Vertical Bar Chart:**
- Top-right: Two metric callout boxes ("MARKET TRENDS: +X% GROWTH", "INVESTMENT: HIGH DEMAND")
- Central: Vertical bar chart with black bars, X-axis=item names, values labeled at tops
- Bottom: Side-by-side bars — black ("MELBOURNE AVG: $X/SQM") + red ("ULTRA-LUXURY RANGE: $X — $X/SQM")

Bar charts MUST use solid black fill. No gradients, no patterns.

#### CRITICAL: Page Numbering and Footer (Every Slide)

- **Red page-number box**: Bottom-left, ~0.4" square, red fill (`#D32F2F`), white number centered
- **Footer text**: Bottom-right, format "{PUBLICATION TITLE} | {PAGE NUMBER}", Liter 9 pt, ALL CAPS, `#333333`
- Cover slide (slide 1) has page number but NO footer text
- All other slides (2–15) have BOTH page number and footer

#### Mandatory Slide Types (Must All Be Present)

1. **Section Dividers** (slides 3, 6): Left text ~40% + red vertical line + right dark image ~60% + caption
2. **Detail Slides** (slides 4, 7): Title + red underline + 3-column (image + specs + quote/price)
3. **Collection Summaries** (slides 5, 8): Comparison cards with property info and features
4. **Architecture Slide** (slide 10): Architect names with red dot bullets + large image + skyline card
5. **Interior/Amenities Slide** (slide 11): Finishes list + 2x3 image grid + amenities list + floor plan thumbnails
6. **Investment Slide** (slide 12): Metrics with mini bar charts + image grid + bottom stat bars
7. **Neighbourhood Slide** (slide 13): 2x2 white neighbourhood cards with red left border + map placeholder with red dots
8. **Floor Plans Slide** (slide 14): 4-column comparison cards with gray placeholders
9. **Closing Slide** (slide 15): Left metrics + mini chart + right image/logo + red contact panel + diagonal accent bars

#### Image Placeholder Rules

- **ALWAYS** use dark near-black rectangles (`#1A1A1A`–`353535`) for all image areas
- **NEVER** attempt to download or embed external images
- Vary the specific shade across different placeholders for visual interest
- Hero images on section dividers: Large dark rectangle filling ~60% of slide
- Grid thumbnails: Smaller dark rectangles (~3.06" x 0.97")
- Floor plan placeholders: Light gray rectangles with red dot markers

#### Price and Button Styling

- Price callouts: Red text (`#D32F2F`), Liter, 18–22 pt, bold
- Price boxes: Red filled rectangle (`#D32F2F`) with white text OR black filled rectangle with white text
- Bottom info bars: Full-width black bar (`#000000`) with white ALL CAPS text
- Accent bottom bars: Red bar (`#D32F2F`) with white text (used alongside black bars)

### 5. Font Handling for CJK Content

When the presentation includes Chinese, Japanese, or Korean text:
- **Headings**: Use Noto Serif SC (or equivalent CJK serif) in place of Oranienbaum
- **Body**: Use Noto Sans SC (or equivalent CJK sans-serif) in place of Liter
- Maintain ALL CAPS heading style where applicable; for CJK use bold weight for emphasis
- Preserve the red accent color `#D32F2F` for emphasis regardless of script

### 6. Final Quality Checklist

Before delivering, verify ALL of the following:
- [ ] 15 slides total (or minimum 5 with all mandatory types represented)
- [ ] Cover has split layout with left text panel + right B&W hero + diagonal accent bars
- [ ] Slide 2 shows grid of ALL items (4x5 or 5x4) with dark placeholders
- [ ] Slides 5 and 9 contain bar charts with solid black bars
- [ ] Every slide has red page-number square box bottom-left
- [ ] Every slide except cover has footer text bottom-right
- [ ] All image placeholders are dark near-black rectangles (no external images)
- [ ] Background is gray (`#CCCCCC`/`#D3D3D3`), NOT cream/beige
- [ ] Accent color is red `#D32F2F` throughout (bullets, prices, lines, boxes)
- [ ] Headings are ALL CAPS Oranienbaum, body is Liter
- [ ] Red underline decoration beneath main slide titles
- [ ] Bottom info bars are black with white text
- [ ] All 9 dedicated slide types (section divider, detail, collection summary, architecture, interiors, investment, neighbourhood, floor plans, closing) are present
