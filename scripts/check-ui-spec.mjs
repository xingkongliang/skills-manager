#!/usr/bin/env node
// Enforces the greppable parts of the UI spec ("UI 风格规范" in AGENTS.md):
// radii and font sizes off their scales, colour literals in class
// names, and palette text colours that only read well in one theme. Each
// rule's message names the fix; exceptions carry their reason here.
import fs from 'node:fs';
import path from 'node:path';

// Agent icon cells are spec'd at rounded-[4px], and their initial-letter
// fallback needs a glyph smaller than the type scale.
const AGENT_ICON_CELLS = new Set([
  'src/components/AddSkillsSheet.tsx',
  'src/components/AgentControlSetupCard.tsx',
  'src/components/AgentToggleSection.tsx',
  'src/components/BatchSyncAgentDialog.tsx',
  'src/components/ProjectAgentDots.tsx',
  'src/components/SkillProjectsSection.tsx',
  'src/components/SyncDots.tsx',
]);
const ICON_GLYPH_SIZES = ['8px', '9px', '10px'];

// The other small square cells that sit beside agent icons share their 4px
// radius: the sidebar's nav, preset and project icons, and custom checkboxes.
const FOUR_PX_CELLS = new Set([
  ...AGENT_ICON_CELLS,
  'src/components/AddProjectDialog.tsx',
  'src/components/Sidebar.tsx',
  'src/components/SkillPickerRow.tsx',
]);

const RADIUS_EXCEPTIONS = {
  // The deliberate welcome dialog.
  'src/components/HelpDialog.tsx': ['rounded-[28px]', 'rounded-2xl'],
  // Inline code inside a rendered SKILL.md is document typography.
  'src/components/SkillMarkdown.tsx': ['rounded'],
  'src/components/SkillReaderMarkdown.tsx': ['rounded'],
};

const TYPE_SCALE = ['11px', '12px', '13px', '14px', '17px'];
const FONT_SIZE_EXCEPTIONS = {
  // Headings of a rendered SKILL.md are document typography, not UI chrome.
  'src/components/SkillMarkdown.tsx': ['28px', '20px', '16px'],
  'src/components/SkillReaderMarkdown.tsx': ['28px', '20px', '16px'],
  // The sheet's title and lead sit directly above that document and match its h1.
  'src/components/DetailSheet.tsx': ['28px', '15px'],
  // The deliberate welcome dialog.
  'src/components/HelpDialog.tsx': ['20px'],
  // The device-login code the user reads and types into GitHub.
  'src/views/Backup.tsx': ['26px'],
};

const COLOUR_LITERAL_EXCEPTIONS = new Set([
  // GitHub's diff palette: each light value is paired with a dark: value,
  // and no token covers added/removed line backgrounds.
  'src/components/DocumentDiffViewer.tsx',
  // The welcome dialog's glow and oversized drop shadow.
  'src/components/HelpDialog.tsx',
  // The switch knob's drop shadow: a black alpha that reads the same in both themes.
  'src/components/ToggleSwitch.tsx',
  // A top highlight that only exists in dark mode.
  'src/components/StatusBanner.tsx',
]);

const RULES = [
  {
    // Bare rounded (4px), rounded-sm, rounded-2xl/3xl and arbitrary values are off the scale.
    pattern: /(?<![\w-])rounded(?:-(?:t|r|b|l|tl|tr|br|bl|s|e|ss|se|es|ee))?(?:-(?:sm|2xl|3xl|\[[^\]\s]+\]))?(?![\w-])/g,
    allowed: (file, match) =>
      (match === 'rounded-[4px]' && FOUR_PX_CELLS.has(file)) ||
      (RADIUS_EXCEPTIONS[file] ?? []).includes(match),
    fix: 'use the radius scale: rounded-xl (cards, panels, dialogs), rounded-lg (buttons, inputs), '
      + 'rounded-md (icon buttons, segments, menu items), rounded-full (pills, tags, badges)',
  },
  {
    pattern: /\btext-(?:\[[\d.]+px\]|(?:xs|sm|base|lg|[2-9]?xl)\b)/g,
    allowed: (file, match) => {
      const size = match.match(/\[(.+)\]/)?.[1];
      return size !== undefined && (
        TYPE_SCALE.includes(size) ||
        (FONT_SIZE_EXCEPTIONS[file] ?? []).includes(size) ||
        (AGENT_ICON_CELLS.has(file) && ICON_GLYPH_SIZES.includes(size))
      );
    },
    fix: 'use a font-size step: text-[17px] page title, text-[14px] card title, text-[13px] body, '
      + 'text-[12px] secondary, text-[11px] pill',
  },
  {
    pattern: /\[[^\]\s"'`]*(?:#[0-9a-fA-F]{3,8}\b|(?:rgba?|hsla?)\()[^\]\s"'`]*\]/g,
    allowed: (file) => COLOUR_LITERAL_EXCEPTIONS.has(file),
    fix: 'use a token class (bg-surface, text-muted, border-border-subtle, text-accent, ...) '
      + 'or a palette class with a dark: variant (ring-amber-500/15, shadow-card, ...)',
  },
];

// Text shades outside 500/600 only read well in one theme: 50–400 vanish on
// the light background, 700–950 on the dark one. They need a dark: partner in
// the same class string.
const PALETTE = 'slate|gray|zinc|neutral|stone|red|orange|amber|yellow|lime|green|emerald|teal|'
  + 'cyan|sky|blue|indigo|violet|purple|fuchsia|pink|rose';
const ONE_THEME_TEXT = new RegExp(
  `(?<![\\w:-])((?:[\\w-]+:)*)(text-(?:${PALETTE})-(?:50|[1-4]00|[7-9]00|950))\\b`,
  'g',
);
const DARK_TEXT = new RegExp(`\\bdark:(?:[\\w-]+:)*text-(?:${PALETTE})-\\d`);
const ONE_THEME_FIX = 'pair it with a dark: variant, e.g. text-amber-600 dark:text-amber-400, '
  + 'or use a 500/600 shade';

const files = fs
  .readdirSync('src', { recursive: true })
  .map((entry) => path.join('src', entry).split(path.sep).join('/'))
  .filter((file) => /\.(tsx?|css)$/.test(file));

const violations = [];
for (const file of files) {
  fs.readFileSync(file, 'utf8').split('\n').forEach((line, index) => {
    const at = `${file}:${index + 1}`;
    for (const rule of RULES) {
      for (const [match] of line.matchAll(rule.pattern)) {
        if (!rule.allowed(file, match)) {
          violations.push(`${at}: ${match} — ${rule.fix}`);
        }
      }
    }
    for (const [literal] of line.matchAll(/"[^"]*"|'[^']*'|`[^`]*`/g)) {
      if (DARK_TEXT.test(literal)) continue;
      for (const [, variants, colour] of literal.matchAll(ONE_THEME_TEXT)) {
        if (!variants.includes('dark:')) {
          violations.push(`${at}: ${colour} — ${ONE_THEME_FIX}`);
        }
      }
    }
  });
}

if (violations.length > 0) {
  console.error(violations.join('\n'));
  console.error(`\n${violations.length} UI spec violation(s).`);
  process.exit(1);
}
console.log(`UI spec check passed (${files.length} files).`);
