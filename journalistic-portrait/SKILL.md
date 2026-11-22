---
name: journalistic-portrait
description: Create magazine-style HTML pages replicating the visual design of Southern People Weekly. Use for feature article layouts, cover pages with red border frames, table-of-contents pages with portrait photography, and dual-column inner pages with editorial section headers, mimicking the typography and color scheme of Chinese weekly magazines.
---

# Southern People Weekly Magazine Layout Skill

Replicates the visual identity of *Southern People Weekly* (南方人物周刊) as HTML web pages.

## Reference Source

- **Type**: Uploaded PDF artifact (native PDF)
- **Reference File Type**: PDF
- **Style origin**: Extracted from concrete 4-page PDF visual analysis
- **Language**: Chinese (Simplified) with occasional English labels

## Supported Outputs

| Format | Supported | Notes |
|--------|-----------|-------|
| HTML (static web page) | Yes | Primary output |
| PDF | Yes | Via print-to-PDF from browser |
| DOCX | No | Not applicable for magazine layout |
| PPTX | No | Not applicable for magazine layout |

**Default output**: HTML static web page (single-file or multi-page).

## Workflow

1. Read `references/style_contract.md` for the complete visual style specification.
2. Read `references/structure_contract.md` for the page structure rules and layout patterns.
3. Generate HTML following all contracts and the **4 Frontend Iron Rules** below.
4. Validate each page type against the checklist in the Iron Rules section.

## The 4 Frontend Iron Rules (MANDATORY — MUST APPEAR VERBATIM)

These 4 rules must be explicitly written into the generated HTML/CSS and strictly followed:

---

### Rule 1: Cover Layering (z-index Stack)

> 封面分层：准确提取原件色彩，切勿将红边框设为全局背景。用 z-index 分三层：底层暗调背景图、中层报头、顶层边框与导读。

The cover page MUST use a 3-layer z-index stack. Never set the red border as a global background.

- Layer 0 (`z-index: 0`): Darkened full-bleed cover photo with gradient overlay
- Layer 1 (`z-index: 1`): Masthead "南方" + "人物周刊" + cover headline text
- Layer 2 (`z-index: 2`): Red border frame + right-side TOC teasers + bottom info bar

**CSS skeleton:**
```css
.cover-container { position: relative; width: 100%; aspect-ratio: 3/4; min-height: 900px; overflow: hidden; }
.cover-bg        { position: absolute; z-index: 0; inset: 20px; }
.cover-bg img    { width: 100%; height: 100%; object-fit: cover; filter: saturate(0.85); }
.cover-bg-overlay { position: absolute; inset: 0; background: linear-gradient(to bottom, rgba(0,0,0,0.1), rgba(0,0,0,0.45)); }
.cover-masthead  { position: absolute; z-index: 1; inset: 20px; display: flex; flex-direction: column; }
.cover-frame     { position: absolute; z-index: 2; inset: 0; border: 12px solid #D90712; pointer-events: none; }
.cover-frame::after { content: ''; position: absolute; inset: 8px; border: 2px solid rgba(255,255,255,0.6); }
.cover-teasers   { position: absolute; z-index: 2; right: 60px; bottom: 120px; color: #fff; text-align: right; }
.cover-bottom-bar { position: absolute; z-index: 2; bottom: 20px; left: 20px; right: 20px; }
```

**Cover checklist:**
- [ ] Masthead reads "南方" + "人物周刊" (NOT "南方人物周刊" as one line)
- [ ] Slogan "时代的肖像" centered at top
- [ ] Right-side teaser boxes present (P62文化, P70娱乐, +P38国际)
- [ ] "国际" teaser has white circle with red "+" icon
- [ ] Bottom info bar with ISSN, barcode, QR code, price, issue info, website
- [ ] Red border `#D90712` exactly 12px, with inner white line

---

### Rule 2: Image-Text Collision Prevention (No Absolute Images)

> 防图文遮挡：内页图片严禁使用 position: absolute。必须用 float 配合 margin 推开文字，父级容器强制应用 clearfix。

Inner-page images MUST NEVER use `position: absolute`. Always use `float` with margin to push text away. Parent container MUST apply clearfix.

```css
.article-image { float: right; margin: 0 0 1em 1.5em; max-width: 48%; }
.article-image.left { float: left; margin: 0 1.5em 1em 0; }
.article-column::after { content: ""; display: table; clear: both; }
```

**Image checklist:**
- [ ] NO `position: absolute` on any inner-page image
- [ ] Images use `float: right` or `float: left` with margin
- [ ] Parent container has clearfix (`::after` with `clear: both`)
- [ ] Caption is right-aligned below image

---

### Rule 3: Layout Collapse Prevention (No Fixed Heights)

> 防排版崩塌：绝对禁止给文本容器写死固定高度。全局强制应用 word-wrap: break-word 与 box-sizing: border-box。

NEVER assign fixed `height` to text containers. Globally enforce `box-sizing` and `word-wrap`.

```css
* { box-sizing: border-box; }
.text-container,
.column,
.article-body { height: auto !important; word-wrap: break-word; overflow-wrap: break-word; }
```

**Layout checklist:**
- [ ] No fixed `height` on any text container
- [ ] `box-sizing: border-box` applied globally
- [ ] `word-wrap: break-word` and `overflow-wrap: break-word` on all text containers
- [ ] Layout uses `min-height` instead of `height` where needed

---

### Rule 4: High-Density Text Fill

> 高密度填充：设定正文 line-height: 1.5 左右。必须硬性规定下游 AI：每页正文需生成 800-1000 字，彻底填满多栏网格，严防底部留白。

Set body text `line-height: 1.5`. Every article page MUST generate **800-1000 characters** of body text per page to completely fill the multi-column grid. Prevent bottom whitespace.

```css
.article-body { line-height: 1.5; font-size: 15px; }
```

**Density checklist:**
- [ ] `line-height: 1.5` on body text
- [ ] Each article page generates 800-1000 characters
- [ ] Both columns filled to near bottom
- [ ] No excessive whitespace at page bottom (target <15% empty)
- [ ] If text is short, expand content or combine pages

---

## Page Type Quick Reference

| Page Type | Key Rule | Critical Check |
|-----------|----------|----------------|
| Cover | Rule 1 (3-layer z-index) | Teasers + bottom bar present |
| Table of Contents | Single page only | All content on one page |
| Article (first page) | Rules 2-4, **dual-column from page 1** | Never single-column |
| Article (continuation) | Rules 2-4 | 800-1000 chars, filled columns |

## Typography Strategy

- **Primary CJK font**: `"PingFang SC", "Noto Sans SC", "Hiragino Sans GB", "Microsoft YaHei", "WenQuanYi Micro Hei", sans-serif`
- **Reference fonts from PDF**: PingFangSC-Regular + Helvetica
- **Heading weight**: 700 (bold)
- **Body weight**: 400 (regular)
- All text elements use the same font stack for CJK compatibility.

## Color Palette

| Token | Hex | Usage |
|-------|-----|-------|
| `--spw-red` | `#D90712` | Cover border, accent marks, decorative bars, end triangle |
| `--spw-red-dark` | `#B0060F` | Hover states |
| `--spw-black` | `#1A1A1A` | Body text, headings |
| `--spw-gray` | `#666666` | Captions, secondary text, author info |
| `--spw-gray-light` | `#999999` | Page numbers, tertiary labels |
| `--spw-white` | `#FFFFFF` | Page background, cover text |
| `--spw-cream` | `#F5F0E8` | Alternate section backgrounds |

## Known Issues to Prevent (from Judge Feedback)

1. **NEVER create blank pages** between cover and TOC or TOC and articles
2. **NEVER use single-column layout** on first page of articles - dual-column always
3. **NEVER omit the cover teasers** (right-side boxes) or bottom info bar
4. **NEVER split TOC across multiple pages** - keep on one page
5. **NEVER leave 50%+ whitespace** at bottom of article pages
6. **ALWAYS place page numbers** consistently (bottom-left or bottom-right)
7. **ALWAYS use `grid-template-columns: 1fr 1fr`** for dual-column, not `column-count`
