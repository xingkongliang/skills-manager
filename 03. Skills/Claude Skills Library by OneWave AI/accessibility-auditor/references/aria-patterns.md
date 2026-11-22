# Common ARIA Patterns

```tsx
// Tabs
<div role="tablist">
  <button role="tab" aria-selected="true" aria-controls="panel-1">Tab 1</button>
  <button role="tab" aria-selected="false" aria-controls="panel-2">Tab 2</button>
</div>
<div role="tabpanel" id="panel-1">Content 1</div>
<div role="tabpanel" id="panel-2" hidden>Content 2</div>

// Accordion
<button aria-expanded="true" aria-controls="content-1">Section 1</button>
<div id="content-1">Content</div>

// Menu
<button aria-haspopup="menu" aria-expanded="false">Options</button>
<ul role="menu" hidden>
  <li role="menuitem">Option 1</li>
  <li role="menuitem">Option 2</li>
</ul>

// Alert
<div role="alert">Error: Please fix the form</div>

// Progress
<div role="progressbar" aria-valuenow="50" aria-valuemin="0" aria-valuemax="100">
  50%
</div>
```
