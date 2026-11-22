# Automated Testing

## CLI Tools

```bash
# Lighthouse accessibility audit
npx lighthouse https://yoursite.com --only-categories=accessibility --view

# axe-core CLI
npx @axe-core/cli https://yoursite.com

# Pa11y
npx pa11y https://yoursite.com
```

## React Testing Library + jest-axe

```typescript
import { render } from '@testing-library/react';
import { axe, toHaveNoViolations } from 'jest-axe';

expect.extend(toHaveNoViolations);

test('Button has no accessibility violations', async () => {
  const { container } = render(<Button>Click me</Button>);
  const results = await axe(container);
  expect(results).toHaveNoViolations();
});
```

## Playwright Accessibility Testing

```typescript
import { test, expect } from '@playwright/test';
import AxeBuilder from '@axe-core/playwright';

test('page has no accessibility violations', async ({ page }) => {
  await page.goto('/');

  const results = await new AxeBuilder({ page }).analyze();
  expect(results.violations).toEqual([]);
});
```

## Tools

- axe DevTools - Browser extension
- WAVE - Browser extension
- Lighthouse - Built into Chrome
- NVDA - Free Windows screen reader
- VoiceOver - Built into macOS (Cmd+F5)
- Color Contrast Analyzer - Desktop app
