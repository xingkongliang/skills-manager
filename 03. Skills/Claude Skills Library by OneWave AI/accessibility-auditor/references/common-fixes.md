# Common Issues and Fixes

## 1. Missing Alt Text

```tsx
// Bad
<img src="/hero.jpg" />

// Good - Informative image
<img src="/hero.jpg" alt="Team collaborating in modern office" />

// Good - Decorative image
<img src="/decoration.jpg" alt="" role="presentation" />

// Good - Icon button
<button aria-label="Close dialog">
  <XIcon aria-hidden="true" />
</button>
```

## 2. Missing Form Labels

```tsx
// Bad
<input type="email" placeholder="Email" />

// Good - Visible label
<div>
  <label htmlFor="email">Email</label>
  <input id="email" type="email" />
</div>

// Good - Visually hidden label
<div>
  <label htmlFor="search" className="sr-only">Search</label>
  <input id="search" type="search" placeholder="Search..." />
</div>
```

## 3. Poor Color Contrast

```tsx
// Bad - 2.5:1 ratio
<p className="text-gray-400 bg-white">Low contrast text</p>

// Good - 4.5:1+ ratio
<p className="text-gray-700 bg-white">Accessible text</p>

// Check contrast: https://webaim.org/resources/contrastchecker/
```

## 4. Missing Focus Styles

```css
/* Bad - Removes focus */
*:focus { outline: none; }

/* Good - Custom focus style */
*:focus-visible {
  outline: 2px solid #3b82f6;
  outline-offset: 2px;
}

/* Tailwind */
.btn {
  @apply focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-blue-500 focus-visible:ring-offset-2;
}
```

## 5. Non-semantic HTML

```tsx
// Bad
<div onClick={handleClick}>Click me</div>

// Good
<button onClick={handleClick}>Click me</button>

// Bad
<div className="header">...</div>

// Good
<header>...</header>
```

## 6. Missing ARIA for Dynamic Content

```tsx
// Loading state
<button disabled aria-busy="true">
  <span className="sr-only">Loading</span>
  <Spinner aria-hidden="true" />
</button>

// Live region for updates
<div aria-live="polite" aria-atomic="true">
  {message && <p>{message}</p>}
</div>

// Modal
<div
  role="dialog"
  aria-modal="true"
  aria-labelledby="modal-title"
>
  <h2 id="modal-title">Dialog Title</h2>
</div>
```

## 7. Skip Link

```tsx
// Add as first focusable element
<a
  href="#main-content"
  className="sr-only focus:not-sr-only focus:absolute focus:top-4 focus:left-4 focus:z-50 focus:px-4 focus:py-2 focus:bg-white"
>
  Skip to main content
</a>

<main id="main-content">
  ...
</main>
```

## Screen Reader Only Class

```css
/* Visually hidden but accessible */
.sr-only {
  position: absolute;
  width: 1px;
  height: 1px;
  padding: 0;
  margin: -1px;
  overflow: hidden;
  clip: rect(0, 0, 0, 0);
  white-space: nowrap;
  border: 0;
}

/* Show on focus (for skip links) */
.sr-only-focusable:focus {
  position: static;
  width: auto;
  height: auto;
  padding: inherit;
  margin: inherit;
  overflow: visible;
  clip: auto;
  white-space: normal;
}
```
